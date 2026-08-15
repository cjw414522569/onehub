//! Local HTTP server that hosts the built mxterm UI (dist) for the WebView2
//! shell (T006). Serves clients/windows/ui/dist on 127.0.0.1 with a random
//! port so the web UI can load ES modules without file:// restrictions.

use std::io;
use std::path::{Path, PathBuf};
use std::thread;

use tiny_http::{Header, Response, Server};

/// Resolves the built UI directory relative to this executable or the cwd.
pub(crate) fn resolve_dist_dir(exe_dir: &Path, cwd: &Path) -> Option<PathBuf> {
    let candidates = [
        exe_dir.join("../../clients/windows/ui/dist"),
        exe_dir.join("../clients/windows/ui/dist"),
        cwd.join("clients/windows/ui/dist"),
    ];
    candidates
        .into_iter()
        .find(|path| path.join("index.html").is_file())
}

/// Starts serving `dist_dir` on 127.0.0.1 with an OS-assigned port and
/// returns the port. The server thread lives for the process lifetime.
pub(crate) fn serve(dist_dir: PathBuf) -> io::Result<u16> {
    let server = Server::http("127.0.0.1:0").map_err(|err| io::Error::other(err.to_string()))?;
    let port = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| io::Error::other("server has no ip addr"))?
        .port();
    thread::spawn(move || {
        for request in server.incoming_requests() {
            let url = request.url().split('?').next().unwrap_or("/");
            let relative = if url == "/" {
                "index.html"
            } else {
                url.trim_start_matches('/')
            };
            let path = dist_dir.join(relative);
            if path.starts_with(&dist_dir) && path.is_file() {
                let mime = mime_for(&path);
                let data = std::fs::read(&path).unwrap_or_default();
                let header = Header::from_bytes(&b"Content-Type"[..], mime.as_bytes())
                    .unwrap_or_else(|_| {
                        Header::from_bytes(&b"Content-Type"[..], &b"application/octet-stream"[..])
                            .unwrap()
                    });
                let _ = request.respond(Response::from_data(data).with_header(header));
            } else {
                let _ = request.respond(Response::from_string("not found").with_status_code(404));
            }
        }
    });
    Ok(port)
}

fn mime_for(path: &Path) -> String {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8".to_string(),
        Some("js") => "text/javascript; charset=utf-8".to_string(),
        Some("css") => "text/css; charset=utf-8".to_string(),
        Some("svg") => "image/svg+xml".to_string(),
        Some("png") => "image/png".to_string(),
        Some("woff2") => "font/woff2".to_string(),
        Some("json") => "application/json".to_string(),
        Some("wasm") => "application/wasm".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}
