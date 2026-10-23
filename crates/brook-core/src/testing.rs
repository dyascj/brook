//! A tiny HTTP/1.1 server for tests, so network code runs against real
//! sockets without touching the internet.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

/// A canned response for one path.
#[derive(Clone, Debug)]
pub(crate) struct Route {
    path: String,
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Route {
    /// A `200 OK` with `body` at `path` (which may include a query).
    pub(crate) fn new(path: &str, body: impl Into<Vec<u8>>) -> Self {
        Self { path: path.into(), status: 200, headers: vec![], body: body.into() }
    }

    pub(crate) fn status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    pub(crate) fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    fn etag(&self) -> Option<&str> {
        self.headers.iter().find(|(name, _)| name.eq_ignore_ascii_case("etag")).map(|(_, v)| v.as_str())
    }
}

/// A server on a random local port that lives as long as the value.
#[derive(Debug)]
pub(crate) struct TestServer {
    base: String,
    hits: Arc<AtomicUsize>,
}

impl TestServer {
    /// Serves `routes`; anything else is a 404. A request whose
    /// `If-None-Match` equals the route's `ETag` gets a 304.
    pub(crate) fn start(routes: Vec<Route>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a local port");
        let base = format!("http://{}", listener.local_addr().unwrap());
        let routes: Arc<HashMap<String, Route>> =
            Arc::new(routes.into_iter().map(|route| (route.path.clone(), route)).collect());
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                counter.fetch_add(1, Ordering::SeqCst);
                let routes = routes.clone();
                thread::spawn(move || serve(stream, &routes));
            }
        });
        Self { base, hits }
    }

    /// The absolute URL of `path` on this server.
    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// How many connections the server has accepted.
    pub(crate) fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

fn serve(stream: TcpStream, routes: &HashMap<String, Route>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let path = request_line.split_whitespace().nth(1).unwrap_or("/").to_owned();
    let mut if_none_match = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("if-none-match")
        {
            if_none_match = Some(value.trim().to_owned());
        }
    }

    let not_found = Route::new(&path, "not found").status(404);
    let route = routes.get(&path).unwrap_or(&not_found);
    let (status, body): (u16, &[u8]) = match (route.etag(), if_none_match.as_deref()) {
        (Some(etag), Some(sent)) if etag == sent => (304, b""),
        _ => (route.status, &route.body),
    };
    let mut response =
        format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
    for (name, value) in &route.headers {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str("\r\n");
    let mut stream = stream;
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(body);
}
