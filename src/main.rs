use std::error::Error;
use std::fs::{File, OpenOptions};
use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::{env, io};

use base64::prelude::*;
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::header::{CONNECTION, UPGRADE};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use input_linux::sys::{input_event, timeval};
use input_linux::{EventKind, InputId, Key, RelativeAxis, UInputHandle};
use qrcode::QrCode;
use qrcode::render::unicode::Dense1x2;
use sha1::{Digest, Sha1};
use tokio::io::{AsyncReadExt, BufReader};
use tokio::net::TcpListener;

fn local_ip() -> String {
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("8.8.8.8:80").and_then(|_| s.local_addr()))
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "localhost".into())
}

fn print_qr(url: &str) -> Result<(), Box<dyn Error>> {
    let qr = QrCode::new(url)?
        .render::<Dense1x2>()
        .dark_color(Dense1x2::Light)
        .light_color(Dense1x2::Dark)
        .quiet_zone(false)
        .build();
    for line in qr.lines() {
        println!("    {line}");
    }
    Ok(())
}

fn create_uinput() -> io::Result<UInputHandle<File>> {
    let ui = UInputHandle::new(OpenOptions::new().write(true).open("/dev/uinput")?);
    ui.set_evbit(EventKind::Relative)?;
    ui.set_relbit(RelativeAxis::WheelHiRes)?;
    ui.set_evbit(EventKind::Key)?;
    ui.set_keybit(Key::ButtonLeft)?; // needed for libinput to recognize it as a mouse
    ui.create(&InputId { bustype: 0x06, vendor: 0, product: 0, version: 0 }, b"wifi-scroll", 0, &[])?; // BUS_VIRTUAL
    Ok(ui)
}

fn emit_scroll(ui: &UInputHandle<File>, ticks: i32) -> io::Result<()> {
    let zero_time = timeval { tv_sec: 0, tv_usec: 0 };
    ui.write(&[
        input_event {
            time: zero_time,
            type_: EventKind::Relative as u16,
            code: RelativeAxis::WheelHiRes as u16,
            value: ticks,
        },
        input_event { time: zero_time, type_: 0, code: 0, value: 0 }, // SYN_REPORT
    ])?;
    Ok(())
}

async fn ws_loop(upgraded: hyper::upgrade::Upgraded, ui: Arc<Mutex<UInputHandle<File>>>) -> io::Result<()> {
    let mut stream = BufReader::new(TokioIo::new(upgraded));
    let mut acc = 0.0;
    let mut buf = [0u8; 2 + 4 + 4]; // header + mask key + f32 payload

    loop {
        stream.read_exact(&mut buf).await?;

        // close frame
        if buf[0] & 0x0F == 0x8 {
            break;
        }

        // Unmask payload (last 4 bytes) with mask key (bytes 2..6)
        for i in 0..4 {
            buf[6 + i] ^= buf[2 + i]
        }

        acc += 6.0 * f32::from_le_bytes(buf[6..10].try_into().unwrap());
        let ticks = acc as i32;
        if ticks != 0 {
            let ui = ui.lock().unwrap();
            emit_scroll(&ui, ticks)?;
            acc -= ticks as f32;
        }
    }
    Ok(())
}

async fn handle(
    req: Request<Incoming>,
    ui: Arc<Mutex<UInputHandle<File>>>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/") => Ok(Response::builder()
            .header("Content-Type", "text/html")
            .header("Cache-Control", "no-store")
            .body(Full::new(Bytes::from(include_str!("index.html"))))
            .unwrap()),

        (&Method::GET, "/ws") => {
            let mut sha1 = Sha1::new();
            sha1.update(req.headers().get("Sec-WebSocket-Key").unwrap().as_bytes());
            sha1.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11"); // RFC 6455 magic GUID
            let accept = BASE64_STANDARD.encode(sha1.finalize());

            tokio::spawn(async move {
                if let Err(e) = ws_loop(hyper::upgrade::on(req).await.unwrap(), ui).await
                    && e.kind() != io::ErrorKind::UnexpectedEof
                {
                    eprintln!("ws error: {e}");
                }
            });

            Ok(Response::builder()
                .status(StatusCode::SWITCHING_PROTOCOLS)
                .header(UPGRADE, "websocket")
                .header(CONNECTION, "Upgrade")
                .header("Sec-WebSocket-Accept", accept)
                .body(Full::default())
                .unwrap())
        }

        _ => Ok(Response::builder().status(StatusCode::NOT_FOUND).body(Full::default()).unwrap()),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let port = args.iter().find_map(|a| a.parse().ok()).unwrap_or(12687);
    let url = format!("http://{}:{port}", local_ip());
    if args.iter().any(|a| a == "-q" || a == "--qr") {
        print_qr(&url)?;
    }
    println!("Listening on {url}");
    let ui = Arc::new(Mutex::new(create_uinput()?));
    let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).await?;
    loop {
        tokio::select! {
            Ok((stream, _)) = listener.accept() => {
                stream.set_nodelay(true)?;
                let ui = ui.clone();
                tokio::spawn(async move {
                    if let Err(e) = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service_fn(move |req| handle(req, ui.clone())))
                        .with_upgrades()
                        .await
                        && !e.is_incomplete_message()
                    {
                        eprintln!("http error: {e}");
                    }
                });
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    Ok(())
}
