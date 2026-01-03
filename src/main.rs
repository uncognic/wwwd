use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
fn client(mut stream: TcpStream) {
    let mut buffer = [0; 512];
    if let Ok(bytes_read) = stream.read(&mut buffer) {
        if bytes_read == 0 {
            return;
        }

        let request = String::from_utf8_lossy(&buffer[..bytes_read]);
        println!("Received request:\n{}", request);


        let first_line = request.lines().next().unwrap_or("");
        let mut served = false;

        if first_line.starts_with("GET / ")
            || first_line.starts_with("GET /index.html ")
            || first_line == "GET / HTTP/1.1"
        {
            match fs::read("index.html") {
                Ok(body) => {
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=UTF-8\r\nContent-Length: {}\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(header.as_bytes());
                    let _ = stream.write_all(&body);
                    let _ = stream.flush();
                    served = true;
                }
                Err(_) => {
                }
            }
        }
        if !served {
            let body = "404 NOT FOUND";
             let header = format!(
                "HTTP/1.1 404 NOT FOUND\r\nContent-Type: text/plain; charset=UTF-8\r\nContent-Length: {}\r\n\r\n",
                body.len()
             );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body.as_bytes());
            let _ = stream.flush();
        }
    }
}

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080")?;
    println!("Server listening on port 8080");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(|| {
                    client(stream);
                });
            }
            Err(e) => {
                eprintln!("Connection failed: {}", e);
            }
        }
    }
    Ok(())
}