//! Lichtspiel, a home streaming server: movies and TV libraries, direct play and
//! transcoding for every video container, playlists (default, manual and
//! smart) with filters and sorting, user invites, plugins and a JSON API.
//!
//!   lichtspiel                       run the server (port 8484 or the one saved in Settings, data in ./data)
//!   lichtspiel --data /config --port 8484
//!   lichtspiel --console             run the server with a status window
//!   lichtspiel --screenshot out.png  draw the status window, save a picture, exit
//!   lichtspiel --self-test out.json  try every feature end to end, save results, exit

#[macro_use]
mod admin;
mod accounts;
mod api;
mod connect;
mod data;
mod library;
mod playback;
mod server;

use std::path::PathBuf;
use std::sync::Arc;

pub struct Options {
    pub data: PathBuf,
    /// None = the port saved in Settings (8484 at first).
    pub port: Option<u16>,
    pub bind: String,
    /// PEM certificate and key for HTTPS; None = what the settings say.
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// LICHTSPIEL_<name>, or the older MEDIASERVER_<name> so existing setups keep working.
pub fn env(name: &str) -> Option<String> {
    std::env::var(format!("LICHTSPIEL_{name}")).or_else(|_| std::env::var(format!("MEDIASERVER_{name}"))).ok()
}

fn options(args: &[String]) -> Options {
    let data = arg_value(args, "--data")
        .or_else(|| env("DATA"))
        .unwrap_or_else(|| "data".into());
    let port = arg_value(args, "--port")
        .or_else(|| env("PORT"))
        .and_then(|p| p.parse().ok());
    let bind = arg_value(args, "--bind").or_else(|| env("BIND")).unwrap_or_else(|| "0.0.0.0".into());
    Options {
        data: PathBuf::from(data), port, bind,
        tls_cert: arg_value(args, "--tls-cert").or_else(|| env("TLS_CERT")),
        tls_key: arg_value(args, "--tls-key").or_else(|| env("TLS_KEY")),
    }
}

/// Opens the library and starts listening. Returns the app and the server.
pub fn start(opts: &Options) -> Result<(Arc<server::App>, Arc<tiny_http::Server>), String> {
    let app = server::App::open(&opts.data);
    { let st = app.settings(); data::db::set_articles(st.sort_ignore_articles, &st.sort_articles); }
    let want = opts.port.unwrap_or_else(|| app.db.lock().unwrap().settings.port);
    // Without --bind, the network card chosen in the settings is used.
    let chosen = app.settings().bind_address.trim().to_string();
    let bind = if opts.bind == "0.0.0.0" && !chosen.is_empty() { chosen } else { opts.bind.clone() };
    let bind = if bind.contains(':') && !bind.starts_with('[') { format!("[{bind}]") } else { bind };
    let tls = {
        let st = app.settings();
        server::read_tls(opts.tls_cert.as_deref().unwrap_or(&st.tls_cert), opts.tls_key.as_deref().unwrap_or(&st.tls_key))?
    };
    let use_tls = tls.is_some();
    let http = match tls {
        Some((certificate, private_key)) => tiny_http::Server::https(format!("{bind}:{want}"), tiny_http::SslConfig { certificate, private_key })
            .map_err(|e| format!("could not listen with HTTPS on {bind}:{want} ({e}); is the key the one of the certificate, and is another server using the port?"))?,
        None => tiny_http::Server::http(format!("{bind}:{want}"))
            .map_err(|e| format!("could not listen on {bind}:{want} ({e}); is another server using that port?"))?,
    };
    server::TLS_ON.store(use_tls, std::sync::atomic::Ordering::Relaxed);
    *server::LISTEN.lock().unwrap() = format!("{bind}:{want}");
    if use_tls {
        let st = app.settings();
        *server::TLS_FILES.lock().unwrap() = (opts.tls_cert.clone().unwrap_or(st.tls_cert), opts.tls_key.clone().unwrap_or(st.tls_key));
    }
    let port = http.server_addr().to_ip().map(|a| a.port()).unwrap_or(want);
    *app.port.lock().unwrap() = port;
    // http:// -> https:// on a second port, when asked for.
    let redirect = app.settings().http_redirect_port;
    if use_tls && redirect != 0 && redirect == port {
        admin::log::add(format!("port {redirect} is the HTTPS port itself, so http:// is not passed on; choose another port"));
    } else if use_tls && redirect != 0 {
        match server::redirect_to_https(&bind, redirect, port) {
            Ok(()) => admin::log::add(format!("http://…:{redirect} is passed on to https://…:{port}")),
            Err(e) => admin::log::add(format!("could not listen on port {redirect} to pass http:// on to https:// ({e})")),
        }
    }
    server::start_background(&app);
    connect::relay::start_agent(app.clone());
    connect::gdm::start(&app);
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
        admin::plugins::example_plugin_main();
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("lichtspiel {}\n\n  --data DIR         where the library, settings and plugins are kept (default ./data, or LICHTSPIEL_DATA)\n  --port N           port to listen on (default 8484 or the one saved in Settings, or LICHTSPIEL_PORT)\n  --bind ADDR        address to listen on (default 0.0.0.0)\n  --console          also open the optional desktop status window\n  --no-browser       don't open the web app in the browser on first start\n  --screenshot FILE  draw the status window, save it as PNG and exit\n  --reset-2fa NAME   switch off the two-step sign-in of a user (when the phone is lost) and exit\n  --self-test FILE   test every feature, write the results as JSON and exit

  --transcode-helper convert video for a Lichtspiel server on this computer (see docs/install.md):
    --helper-secret S  the secret the server is given too (at least 8 characters)
    --helper-listen A  address and port to listen on (default 0.0.0.0:8486)
    --ffmpeg PATH      ffmpeg to use (default ffmpeg on the PATH)
    --hw auto|none|vaapi|qsv|nvenc, --hw-device D   the graphics chip to convert with
    --helper-dir DIR   work folder for converted parts (default in the temp folder)", server::VERSION);
        return;
    }
    // `--relay`: the helper for reaching a home server from outside (runs on a computer on the internet).
    if args.iter().any(|a| a == "--relay") {
        let public = arg_value(&args, "--relay-public").or_else(|| env("RELAY_PUBLIC")).unwrap_or_else(|| "0.0.0.0:8484".into());
        let tunnel = arg_value(&args, "--relay-tunnel").or_else(|| env("RELAY_TUNNEL")).unwrap_or_else(|| "0.0.0.0:8485".into());
        let secret = arg_value(&args, "--relay-secret").or_else(|| env("RELAY_SECRET")).unwrap_or_default();
        if let Err(e) = connect::relay::run_relay(&public, &tunnel, &secret) { eprintln!("{e}"); std::process::exit(1); }
        return;
    }
    // `--transcode-helper`: converts video for a Lichtspiel server (runs on a computer with a strong graphics chip).
    if args.iter().any(|a| a == "--transcode-helper") {
        let o = playback::helper::HelperOptions {
            listen: arg_value(&args, "--helper-listen").or_else(|| env("HELPER_LISTEN")).unwrap_or_else(|| format!("0.0.0.0:{}", playback::helper::DEFAULT_PORT)),
            secret: arg_value(&args, "--helper-secret").or_else(|| env("HELPER_SECRET")).unwrap_or_default(),
            ffmpeg: arg_value(&args, "--ffmpeg").or_else(|| env("FFMPEG")).unwrap_or_else(|| "ffmpeg".into()),
            hw: arg_value(&args, "--hw").or_else(|| env("HW")).unwrap_or_else(|| "auto".into()),
            hw_device: arg_value(&args, "--hw-device").or_else(|| env("HW_DEVICE")).unwrap_or_default(),
            dir: arg_value(&args, "--helper-dir").or_else(|| env("HELPER_DIR")).map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("lichtspiel-helper")),
        };
        if let Err(e) = playback::helper::run(o) { eprintln!("{e}"); std::process::exit(1); }
        return;
    }
    if let Some(out) = arg_value(&args, "--self-test") {
        let passed = admin::selftest::run(&PathBuf::from(out));
        std::process::exit(if passed { 0 } else { 1 });
    }
    let opts = options(&args);
    // Locked out of the only admin account? `--reset-2fa NAME` switches its two-step sign-in off.
    if let Some(name) = arg_value(&args, "--reset-2fa") {
        let app = server::App::open(&opts.data);
        let found = {
            let mut db = app.db.lock().unwrap();
            match db.users.iter_mut().find(|u| u.username.eq_ignore_ascii_case(name.trim())) {
                Some(u) => { u.totp_secret.clear(); u.totp_pending.clear(); true }
                None => false,
            }
        };
        if found { app.save(); println!("Two-step sign-in is off for {name}."); } else { eprintln!("There is no user called {name}."); std::process::exit(1); }
        return;
    }
    if let Some(out) = arg_value(&args, "--screenshot") {
        #[cfg(feature = "console")]
        {
            admin::console::screenshot(PathBuf::from(out), opts);
            return;
        }
        #[cfg(not(feature = "console"))]
        {
            eprintln!("this build has no window; build with the 'console' feature to save {out}");
            std::process::exit(2);
        }
    }
    let _ = std::fs::create_dir_all(&opts.data);
    admin::log::init(&opts.data);
    let (app, http) = match start(&opts) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let port = *app.port.lock().unwrap();
    println!("{} {} is running.", server::APP_NAME, server::VERSION);
    let scheme = server::scheme();
    println!("  Open {scheme}://{}:{port}/ in a browser (or {scheme}://localhost:{port}/ on this computer).", lan_address());
    println!("  Data folder: {}", std::fs::canonicalize(&opts.data).unwrap_or(opts.data.clone()).display());
    println!("  API list: GET /api/docs");
    admin::log::add(format!("{} {} started", server::APP_NAME, server::VERSION));
    // The web app is the interface. The desktop status window is optional:
    // only with --console (the Synology/Docker build has none at all).
    let want_window = args.iter().any(|a| a == "--console");
    // First start on a desktop computer: open the web app so the owner account can be made.
    let setup_needed = app.db.lock().unwrap().users.is_empty();
    if setup_needed && (cfg!(windows) || cfg!(target_os = "macos") || want_window) && !args.iter().any(|a| a == "--no-browser") {
        open_browser(&format!("{scheme}://localhost:{port}/"));
    }
    #[cfg(feature = "console")]
    if want_window {
        let a = app.clone();
        std::thread::spawn(move || server::serve(a, http));
        admin::console::live(app);
        return;
    }
    let _ = want_window;
    server::serve(app, http);
}
