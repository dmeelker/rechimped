use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub async fn serve(responses: Vec<(&'static str, String)>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api/", listener.local_addr().unwrap());
    let requests = tokio::spawn(async move {
        let mut requests = Vec::new();
        for (status, body) in responses {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            let header_end = loop {
                if let Some(i) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i + 4;
                }
                let n = socket.read(&mut buf).await.unwrap();
                assert!(n > 0, "connection closed before the headers were complete");
                request.extend_from_slice(&buf[..n]);
            };
            let headers = String::from_utf8(request[..header_end].to_vec()).unwrap().to_lowercase();
            let length = headers
                .lines()
                .find_map(|l| l.strip_prefix("content-length: "))
                .map_or(0, |n| n.trim().parse::<usize>().unwrap());
            while request.len() < header_end + length {
                let n = socket.read(&mut buf).await.unwrap();
                assert!(n > 0, "connection closed before the body was complete");
                request.extend_from_slice(&buf[..n]);
            }
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            requests.push(headers + std::str::from_utf8(&request[header_end..]).unwrap());
        }
        requests
    });
    (base, requests)
}
