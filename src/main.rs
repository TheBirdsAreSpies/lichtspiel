//! Home media server: movies and TV libraries, direct play and
//! transcoding for every video container, playlists (default, manual and
//! smart) with filters and sorting, user invites, plugins and a JSON API.
//!
//!   mediaserver                      run the server (port 8484 or the one saved in Settings, data in ./data)
//!   mediaserver --data /config --port 8484
//!   mediaserver --console            run the server with a status window
//!   mediaserver --screenshot out.png draw the status window, save a picture, exit
//!   mediaserver --self-test out.json try every feature end to end, save results, exit

mod auth;
mod db;
mod handlers;
mod metadata;
mod migrate;
mod plugins;
mod query;
mod routes;
mod scanner;
mod selftest;
mod server;
mod transcode;
#[cfg(feature = "console")]
mod console;

use std::path::PathBuf;
use std::sync::Arc;

pub struct Options {
    pub data: PathBuf,
    /// None = the port saved in Settings (8484 at first).
    pub port: Option<u16>,
    pub bind: String,
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn options(args: &[String]) -> Options {
    let data = arg_value(args, "--data")
        .or_else(|| std::env::var("MEDIASERVER_DATA").ok())
        .unwrap_or_else(|| "data".into());
    let port = arg_value(args, "--port")
        .or_else(|| std::env::var("MEDIASERVER_PORT").ok())
        .and_then(|p| p.parse().ok());
    let bind = arg_value(args, "--bind").or_else(|| std::env::var("MEDIASERVER_BIND").ok()).unwrap_or_else(|| "0.0.0.0".into());
    Options { data: PathBuf::from(data), port, bind }
}

/// Opens the library and starts listening. Returns the app and the server.
pub fn start(opts: &Options) -> Result<(Arc<server::App>, Arc<tiny_http::Server>), String> {
    let app = server::App::open(&opts.data);
    let want = opts.port.unwrap_or_else(|| app.db.lock().unwrap().settings.port);
    let http = tiny_http::Server::http(format!("{}:{}", opts.bind, want))
        .map_err(|e| format!("could not listen on {}:{} ({e}); is another server using that port?", opts.bind, want))?;
    let port = http.server_addr().to_ip().map(|a| a.port()).unwrap_or(want);
    *app.port.lock().unwrap() = port;
    server::start_background(&app);
    app.fire("server.started", serde_json::json!({"port": port}));
    Ok((app, Arc::new(http)))
}

/// Opens a web address in the computer's normal browser (doesn't wait for it).
#[allow(dead_code)]
pub fn open_browser(url: &str) -> bool {
    use std::process::{Command, Stdio};
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", url]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().is_ok()
}

pub fn lan_address() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("10.255.255.255:1")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "localhost".into())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--example-plugin") {
        plugins::example_plugin_main();
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("mediaserver {}\n\n  --data DIR         where the library, settings and plugins are kept (default ./data, or MEDIASERVER_DATA)\n  --port N           port to listen on (default 8484 or the one saved in Settings, or MEDIASERVER_PORT)\n  --bind ADDR        address to listen on (default 0.0.0.0)\n  --console          also open the optional desktop status window\n  --no-browser       don't open the web app in the browser on first start\n  --screenshot FILE  draw the status window, save it as PNG and exit\n  --self-test FILE   test every feature, write the results as JSON and exit", server::VERSION);
        return;
    }
    if let Some(out) = arg_value(&args, "--self-test") {
        let passed = selftest::run(&PathBuf::from(out));
        std::process::exit(if passed { 0 } else { 1 });
    }
    let opts = options(&args);
    if let Some(out) = arg_value(&args, "--screenshot") {
        #[cfg(feature = "console")]
        {
            console::screenshot(PathBuf::from(out), opts);
            return;
        }
        #[cfg(not(feature = "console"))]
        {
            eprintln!("this build has no window; build with the 'console' feature to save {out}");
            std::process::exit(2);
        }
    }
    let (app, http) = match start(&opts) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let port = *app.port.lock().unwrap();
    println!("Media server {} is running.", server::VERSION);
    println!("  Open http://{}:{port}/ in a browser (or http://localhost:{port}/ on this computer).", lan_address());
    println!("  Data folder: {}", std::fs::canonicalize(&opts.data).unwrap_or(opts.data.clone()).display());
    println!("  API list: GET /api/docs");
    // The web app is the interface. The desktop status window is optional:
    // only with --console (the Synology/Docker build has none at all).
    let want_window = args.iter().any(|a| a == "--console");
    // First start on a desktop computer: open the web app so the owner account can be made.
    let setup_needed = app.db.lock().unwrap().users.is_empty();
    if setup_needed && (cfg!(windows) || cfg!(target_os = "macos") || want_window) && !args.iter().any(|a| a == "--no-browser") {
        open_browser(&format!("http://localhost:{port}/"));
    }
    #[cfg(feature = "console")]
    if want_window {
        let a = app.clone();
        std::thread::spawn(move || server::serve(a, http));
        console::live(app);
        return;
    }
    let _ = want_window;
    server::serve(app, http);
}
