// tinypit - a tiny zero-window TCP tarpit.
//
// Mechanic: complete the handshake, set a tiny SO_RCVBUF, then NEVER read. The
// attacker's probe/exploit fills our tiny receive buffer, the kernel advertises a
// zero window, and their TCP stack jams in persist-mode. We only ever dribble a
// single space byte occasionally to keep some scanners "waiting for a banner" and to
// notice when a peer finally gives up. We never touch their bytes -> zero parser
// attack surface.
//
// Security note: This binary performs NO reads on accepted TCP connections and
// accepts NO untrusted input after startup. CLI flags are processed only at
// process start before any networking. Configuration is environment-only.
//
// CLI (startup only):
//   tinypit --version     prints version
//   tinypit /?            prints version + "no flags" message
//   tinypit --help        same
//
// Env config (see README):
//   TINYPIT_LISTEN, TINYPIT_MAX, TINYPIT_PER_IP, TINYPIT_DRIBBLE,
//   TINYPIT_RCVBUF, TINYPIT_LOG, TINYPIT_LOG_FORMAT (text|json)

use std::net::SocketAddr;

use tokio::net::TcpListener;

mod config;
mod limits;
mod conn;

use config::Config;
use limits::Limits;
use conn::{hold, log_conn};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    // Process CLI queries *only* at startup. Never on the TCP path.
    // This adds no runtime attack surface or read surface on connections.
    let args: Vec<String> = std::env::args().collect();
    if args
        .iter()
        .any(|a| matches!(a.as_str(), "--version" | "-V" | "/?" | "--help" | "-h"))
    {
        let v = config::version();
        println!("tinypit {}", v);
        println!("There are no flags available in version {}.", v);
        println!("Configuration is via environment variables only.");
        std::process::exit(0);
    }

    let cfg: Config = config::load();

    let addr: SocketAddr = cfg.listen.parse().expect("bad TINYPIT_LISTEN");
    let sock = socket2::Socket::new(
        socket2::Domain::for_address(addr),
        socket2::Type::STREAM,
        None,
    )
    .expect("socket");
    sock.set_reuse_address(true).ok();
    if addr.is_ipv6() {
        // dual-stack: one [::] listener also accepts v4-mapped peers.
        // (*BSD defaults to v6-only; force it off.)
        sock.set_only_v6(false).ok();
    }
    sock.set_recv_buffer_size(cfg.rcvbuf).ok();
    sock.bind(&addr.into()).expect("bind");
    sock.listen(1024).expect("listen");
    sock.set_nonblocking(true).expect("nonblock");
    let listener = TcpListener::from_std(sock.into()).expect("listener");

    let limits = Limits::new(cfg.max_conns, cfg.per_ip_max);
    limits.spawn_status_task();

    eprintln!(
        "[tinypit] listen={} max={} per_ip={} dribble={}s rcvbuf={}B log={} format={}",
        cfg.listen,
        cfg.max_conns,
        cfg.per_ip_max,
        cfg.dribble,
        cfg.rcvbuf,
        cfg.logpath,
        cfg.log_format
    );

    let dribble = cfg.dribble;

    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(x) => x,
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            }
        };
        let ip = peer.ip();

        let permit = match limits.try_acquire(ip) {
            Some(p) => p,
            None => {
                drop(stream);
                continue;
            }
        };

        log_conn(&cfg.logpath, ip, &cfg.log_format);

        let limits2 = limits.clone();
        tokio::spawn(async move {
            let _permit = permit; // held for the whole connection lifetime
            hold(stream, dribble).await;
            limits2.release(ip);
        });
    }
}
