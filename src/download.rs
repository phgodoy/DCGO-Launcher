use anyhow::{Context, Result};
use reqwest::blocking::Client;
use reqwest::{
    StatusCode,
    header::{ETAG, IF_NONE_MATCH},
};
use std::{
    io::{Read, Write},
    path::Path,
    time::Duration,
};
use tempfile::NamedTempFile;

fn http_client() -> Result<Client> {
    Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .connect_timeout(Duration::from_secs(15))
        // Large downloads on slow connections must not hit a total request timeout.
        .timeout(None)
        .build()
        .context("Could not create HTTP client")
}

/// Stream the ZIP to a temporary file in `temp_dir` on the installation disk.
/// Avoid /tmp, which may use RAM on Linux. Return None when already up to date.
pub fn download_update(
    url: &str,
    saved_etag: Option<&str>,
    temp_dir: &Path,
) -> Result<Option<(NamedTempFile, String)>> {
    let client = http_client()?;
    let mut request = client.get(url);
    if let Some(etag) = saved_etag {
        request = request.header(IF_NONE_MATCH, etag);
    }
    let mut response = request
        .send()
        .context("Could not download update")?
        .error_for_status()
        .context("The server returned an error")?;

    if response.status() == StatusCode::NOT_MODIFIED {
        anyhow::ensure!(
            saved_etag.is_some(),
            "The server returned 304 without a saved ETag"
        );
        return Ok(None);
    }
    anyhow::ensure!(
        response.status() == StatusCode::OK,
        "Unexpected server response: {}",
        response.status()
    );
    let etag = response
        .headers()
        .get(ETAG)
        .context("The server did not provide an ETag; download cancelled")?
        .to_str()
        .context("The server returned an invalid ETag")?
        .to_owned();
    anyhow::ensure!(
        !etag.is_empty(),
        "The server returned an empty ETag; download cancelled"
    );
    if saved_etag == Some(etag.as_str()) {
        return Ok(None);
    }

    println!("Downloading update...");
    let total = response.content_length();
    let mut file =
        NamedTempFile::new_in(temp_dir).context("Could not create temporary download file")?;

    let mut buf = [0u8; 64 * 1024];
    let mut downloaded = 0u64;
    let mut last_shown = u64::MAX;
    loop {
        let n = response
            .read(&mut buf)
            .context("Could not read downloaded ZIP")?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .context("Could not write downloaded ZIP")?;
        downloaded += n as u64;
        show_progress(downloaded, total, &mut last_shown);
    }
    file.flush().context("Could not write downloaded ZIP")?;
    println!();

    if let Some(total) = total {
        anyhow::ensure!(
            downloaded == total,
            "Incomplete download: {downloaded} of {total} bytes"
        );
    }

    Ok(Some((file, etag)))
}

/// Refresh only when the displayed value changes (1% or 1 MB).
fn show_progress(downloaded: u64, total: Option<u64>, last_shown: &mut u64) {
    let (value, text) = match total {
        Some(total) if total > 0 => {
            let pct = downloaded * 100 / total;
            (pct, format!("{pct}%"))
        }
        _ => {
            let mb = downloaded / (1024 * 1024);
            (mb, format!("{mb} MB"))
        }
    };
    if value != *last_shown {
        *last_shown = value;
        print!("\rDownloading... {text}   ");
        let _ = std::io::stdout().flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    fn serve(response: &'static str) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                assert_eq!(stream.read(&mut byte).unwrap(), 1);
                request.push(byte[0]);
            }
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8(request).unwrap()
        });
        (url, handle)
    }

    #[test]
    fn unchanged_version_skips_download() {
        let tmp = tempfile::tempdir().unwrap();
        let (url, server) = serve("HTTP/1.1 304 Not Modified\r\nConnection: close\r\n\r\n");
        assert!(
            download_update(&url, Some("\"v1\""), tmp.path())
                .unwrap()
                .is_none()
        );
        assert!(
            server
                .join()
                .unwrap()
                .to_lowercase()
                .contains("if-none-match: \"v1\"")
        );
    }

    #[test]
    fn changed_or_first_version_downloads() {
        let tmp = tempfile::tempdir().unwrap();
        for saved in [None, Some("\"v1\"")] {
            let (url, server) = serve(
                "HTTP/1.1 200 OK\r\nETag: \"v2\"\r\nContent-Length: 3\r\nConnection: close\r\n\r\nzip",
            );
            let (file, etag) = download_update(&url, saved, tmp.path()).unwrap().unwrap();
            assert_eq!(std::fs::read(file.path()).unwrap(), b"zip");
            assert_eq!(etag, "\"v2\"");
            server.join().unwrap();
        }
    }

    #[test]
    fn same_etag_skips_even_when_server_returns_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let (url, server) = serve(
            "HTTP/1.1 200 OK\r\nETag: \"v1\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        assert!(
            download_update(&url, Some("\"v1\""), tmp.path())
                .unwrap()
                .is_none()
        );
        server.join().unwrap();
    }

    #[test]
    fn missing_etag_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let (url, server) =
            serve("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        assert!(
            download_update(&url, None, tmp.path())
                .unwrap_err()
                .to_string()
                .contains("did not provide an ETag")
        );
        server.join().unwrap();
    }

    #[test]
    fn downloaded_file_is_created_in_the_given_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let (url, server) = serve(
            "HTTP/1.1 200 OK\r\nETag: \"v2\"\r\nContent-Length: 3\r\nConnection: close\r\n\r\nzip",
        );
        let (file, _) = download_update(&url, None, tmp.path()).unwrap().unwrap();
        assert_eq!(file.path().parent().unwrap(), tmp.path());
        server.join().unwrap();
    }
}
