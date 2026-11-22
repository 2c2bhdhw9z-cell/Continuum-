//! Continuum's own way to send games from a phone: a small web page on the TV, at an address
//! with a random code in it, shown as a QR code. It only runs while the player wants it, and
//! stops by itself 15 minutes after its last use.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const PORT: u16 = 8770;
const MAX_FILE: u64 = 512 << 20;
const KEEP_FREE: u64 = 300 << 20;

#[derive(Clone)]
pub struct Received {
    pub name: String,
    pub done: u64,
    pub total: u64,
    pub finished: bool,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Shared {
    pub files: Vec<Received>,
    pub last_use: Option<Instant>,
    pub stop: bool,
    pub changed: bool,
}

pub struct Upload {
    pub url: String,
    pub shared: Arc<Mutex<Shared>>,
}

fn random_code() -> String {
    let mut b = [0u8; 8];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut b);
    }
    let letters = b"abcdefghjkmnpqrstuvwxyz";
    b.iter().take(5).map(|x| letters[*x as usize % letters.len()] as char).collect()
}

/// The TV's address on the home network.
pub fn lan_ip() -> Option<String> {
    unsafe {
        let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifap) != 0 {
            return None;
        }
        let mut best: Option<String> = None;
        let mut p = ifap;
        while !p.is_null() {
            let a = (*p).ifa_addr;
            if !a.is_null() && (*a).sa_family as i32 == libc::AF_INET {
                let sin = a as *const libc::sockaddr_in;
                let ip = u32::from_be((*sin).sin_addr.s_addr);
                let s = format!("{}.{}.{}.{}", ip >> 24, (ip >> 16) & 255, (ip >> 8) & 255, ip & 255);
                let private = s.starts_with("192.168.") || s.starts_with("10.") || s.starts_with("172.");
                if !s.starts_with("127.") && (best.is_none() || private) {
                    best = Some(s);
                }
            }
            p = (*p).ifa_next;
        }
        libc::freeifaddrs(ifap);
        best
    }
}

impl Upload {
    pub fn start(games: PathBuf) -> Result<Upload, String> {
        let listener = TcpListener::bind(("0.0.0.0", PORT)).map_err(|e| format!("can't open the transfer page: {e}"))?;
        listener.set_nonblocking(true).ok();
        let code = random_code();
        let ip = lan_ip().unwrap_or_else(|| "this TV".into());
        let url = format!("http://{ip}:{PORT}/{code}");
        let shared = Arc::new(Mutex::new(Shared { last_use: Some(Instant::now()), ..Default::default() }));
        let sh = shared.clone();
        let c = code.clone();
        std::thread::spawn(move || {
            loop {
                {
                    let g = sh.lock().unwrap();
                    if g.stop || g.last_use.map(|t| t.elapsed() > Duration::from_secs(15 * 60)).unwrap_or(false) {
                        break;
                    }
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        let sh2 = sh.clone();
                        let c2 = c.clone();
                        let g2 = games.clone();
                        std::thread::spawn(move || {
                            let _ = handle(stream, &c2, &g2, &sh2);
                        });
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(150)),
                }
            }
            crate::logln!("transfer page stopped");
        });
        crate::logln!("transfer page open at {url}");
        Ok(Upload { url, shared })
    }

    pub fn running(&self) -> bool {
        let g = self.shared.lock().unwrap();
        !g.stop && g.last_use.map(|t| t.elapsed() < Duration::from_secs(15 * 60)).unwrap_or(true)
    }

    pub fn stop(&self) {
        self.shared.lock().unwrap().stop = true;
    }

}

fn respond(s: &mut TcpStream, status: &str, kind: &str, body: &[u8]) -> std::io::Result<()> {
    write!(
        s,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    s.write_all(body)
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn clean_name(n: &str) -> Option<String> {
    let base = n.rsplit(['/', '\\']).next().unwrap_or("").trim();
    let base: String = base.chars().filter(|c| !c.is_control()).collect();
    if base.is_empty() || base.starts_with('.') || base.len() > 200 {
        return None;
    }
    let ext = crate::files::ext_of(&base);
    if crate::files::ROM_EXTS.contains(&ext.as_str()) || crate::files::ARCHIVE_EXTS.contains(&ext.as_str()) {
        Some(base)
    } else {
        None
    }
}

fn handle(mut stream: TcpStream, code: &str, games: &PathBuf, sh: &Arc<Mutex<Shared>>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(60)))?;
    let mut r = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }
    let (method, target) = (parts[0], parts[1]);
    let mut len: u64 = 0;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 {
            break;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let base = format!("/{code}");
    if path != base && path != format!("{base}/") && path != format!("{base}/upload") {
        return respond(&mut stream, "404 Not Found", "text/plain", b"This address isn't open. Use the code on the TV.");
    }
    sh.lock().unwrap().last_use = Some(Instant::now());
    if method == "GET" {
        return respond(&mut stream, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes());
    }
    if method != "PUT" && method != "POST" {
        return respond(&mut stream, "405 Method Not Allowed", "text/plain", b"");
    }
    let name = query.split('&').find_map(|kv| kv.strip_prefix("name=")).map(url_decode).unwrap_or_default();
    let name = match clean_name(&name) {
        Some(n) => n,
        None => return respond(&mut stream, "415 Unsupported Media Type", "text/plain", b"That isn't a Game Boy game (.gb, .gbc, .gba, or a .zip or .7z of one)."),
    };
    if len == 0 || len > MAX_FILE {
        return respond(&mut stream, "413 Payload Too Large", "text/plain", b"That file is too big for a Game Boy game.");
    }
    if crate::files::free_space(games) < len + KEEP_FREE {
        return respond(&mut stream, "507 Insufficient Storage", "text/plain", b"The TV is nearly full.");
    }
    let idx = {
        let mut g = sh.lock().unwrap();
        g.files.push(Received { name: name.clone(), done: 0, total: len, finished: false, error: None });
        g.files.len() - 1
    };
    let part = games.join(format!(".{name}.part"));
    let fin = games.join(&name);
    let mut f = std::fs::File::create(&part)?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut left = len;
    let mut err = None;
    while left > 0 {
        let want = (buf.len() as u64).min(left) as usize;
        match r.read(&mut buf[..want]) {
            Ok(0) => {
                err = Some("the upload stopped part way".to_string());
                break;
            }
            Ok(n) => {
                if let Err(e) = f.write_all(&buf[..n]) {
                    err = Some(e.to_string());
                    break;
                }
                left -= n as u64;
                let mut g = sh.lock().unwrap();
                g.files[idx].done = len - left;
                g.last_use = Some(Instant::now());
            }
            Err(e) => {
                err = Some(e.to_string());
                break;
            }
        }
    }
    drop(f);
    let mut g = sh.lock().unwrap();
    if let Some(e) = err {
        let _ = std::fs::remove_file(&part);
        g.files[idx].error = Some(e);
        drop(g);
        return respond(&mut stream, "500 Internal Server Error", "text/plain", b"The upload didn't finish. Try again.");
    }
    std::fs::rename(&part, &fin)?;
    g.files[idx].finished = true;
    g.changed = true;
    drop(g);
    crate::logln!("received {name} ({len} bytes)");
    respond(&mut stream, "200 OK", "text/plain", b"ok")
}

const PAGE: &str = r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Continuum</title><style>
body{margin:0;min-height:100vh;background:#131316;color:#fff;font:17px -apple-system,system-ui,sans-serif;display:flex;flex-direction:column;align-items:center}
.glow{position:fixed;inset:-20%;background:radial-gradient(circle at 70% 20%,rgba(242,178,51,.35),transparent 45%),radial-gradient(circle at 25% 80%,rgba(255,143,107,.25),transparent 45%);z-index:-1}
main{width:min(92vw,460px);padding:40px 0}
h1{font-weight:300;letter-spacing:4px;font-size:22px;margin:0 0 28px}
.card{background:rgba(255,255,255,.06);border:.5px solid rgba(255,255,255,.1);border-radius:20px;padding:22px}
p{color:rgba(255,255,255,.6);margin:0 0 18px;line-height:1.4}
label{display:block;text-align:center;padding:14px;border-radius:999px;background:linear-gradient(135deg,#FFD66B,#F2B233);color:#000;font-weight:600;box-shadow:0 3px 10px rgba(242,178,51,.35)}
input{display:none}.row{margin-top:14px;font-size:15px}.bar{height:6px;border-radius:3px;background:rgba(255,255,255,.12);margin-top:6px;overflow:hidden}
.bar i{display:block;height:100%;width:0;background:linear-gradient(90deg,#FFD66B,#F2B233)}.ok{color:#9BE15D}.bad{color:#ED294A}
</style></head><body><div class="glow"></div><main><h1>CONTINUUM</h1><div class="card">
<p>Pick your Game Boy games, or a .zip of one. They show up on the TV as soon as they land.</p>
<label>Pick games<input id="f" type="file" multiple></label><div id="list"></div></div></main>
<script>
const list=document.getElementById('list');
document.getElementById('f').onchange=async e=>{for(const file of e.target.files){
 const row=document.createElement('div');row.className='row';row.innerHTML='<span></span><div class="bar"><i></i></div>';
 row.firstChild.textContent=file.name;list.appendChild(row);const bar=row.querySelector('i');
 await new Promise(res=>{const x=new XMLHttpRequest();x.open('PUT',location.pathname.replace(/\/$/,'')+'/upload?name='+encodeURIComponent(file.name));
 x.upload.onprogress=p=>{if(p.lengthComputable)bar.style.width=(p.loaded/p.total*100)+'%'};
 x.onload=()=>{if(x.status==200){bar.style.width='100%';row.firstChild.className='ok';row.firstChild.textContent=file.name+' is on the TV'}else{row.firstChild.className='bad';row.firstChild.textContent=file.name+': '+x.responseText}res()};
 x.onerror=()=>{row.firstChild.className='bad';row.firstChild.textContent=file.name+": didn't send. Is the phone on the same Wi-Fi?";res()};x.send(file)})}e.target.value=''};
</script></body></html>"#;

/// The QR code for an address, as rows of dark/light modules.
pub fn qr(text: &str) -> Option<(usize, Vec<bool>)> {
    let q = qrcodegen::QrCode::encode_text(text, qrcodegen::QrCodeEcc::Medium).ok()?;
    let n = q.size() as usize;
    let mut m = vec![false; n * n];
    for y in 0..n {
        for x in 0..n {
            m[y * n + x] = q.get_module(x as i32, y as i32);
        }
    }
    Some((n, m))
}
