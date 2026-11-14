//! Shared look for the device setup page and the server dashboard.

pub const STYLE: &str = r#"
:root{--bg:#0b0f1a;--panel:#131a2a;--panel2:#1a2337;--text:#e8edf7;--muted:#9aa6bf;--line:#26314a;
--accent:#4c8dff;--accent2:#7a5cff;--ok:#2ecc71;--warn:#f5b041;--bad:#ff6b6b;color-scheme:dark}
@media (prefers-color-scheme: light){:root{--bg:#f4f6fb;--panel:#fff;--panel2:#eef2f9;--text:#0f1626;
--muted:#5b6780;--line:#dde3ef;--bad:#d64545;color-scheme:light}}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--text);font:16px/1.5 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif}
a{color:var(--accent)}
.wrap{max-width:920px;margin:0 auto;padding:24px 16px 64px}
.top{display:flex;align-items:center;gap:12px;margin-bottom:24px}
.top .grow{flex:1}
.logo{width:38px;height:38px;border-radius:11px;background:linear-gradient(135deg,var(--accent),var(--accent2));
display:grid;place-items:center;color:#fff;font-weight:800}
h1{font-size:22px;margin:0;line-height:1.2}
h2{font-size:18px;margin:0 0 12px}
.card{background:var(--panel);border:1px solid var(--line);border-radius:14px;padding:20px;margin:0 0 16px}
.muted{color:var(--muted)}
.small{font-size:14px}
.row{display:flex;gap:10px;flex-wrap:wrap;align-items:center}
.btn{appearance:none;border:1px solid transparent;border-radius:10px;padding:10px 16px;font:inherit;font-weight:600;
background:var(--accent);color:#fff;cursor:pointer;text-decoration:none;display:inline-block;line-height:1.2}
.btn.secondary{background:var(--panel2);color:var(--text);border-color:var(--line)}
.btn.danger{background:transparent;color:var(--bad);border-color:var(--bad)}
form.inline{display:inline}
input,textarea,select{width:100%;font:inherit;color:var(--text);background:var(--panel2);border:1px solid var(--line);
border-radius:10px;padding:10px 12px}
textarea{min-height:180px;font-family:ui-monospace,Menlo,monospace;font-size:13px}
input[type=checkbox],input[type=radio]{width:auto;margin:0 8px 0 0}
label{display:block;font-weight:600;margin:14px 0 6px}
label.check{display:flex;align-items:center;font-weight:500}
.dot{width:10px;height:10px;border-radius:50%;display:inline-block;background:var(--muted);flex:none}
.dot.ok{background:var(--ok)}.dot.warn{background:var(--warn)}.dot.bad{background:var(--bad)}
.code{font:700 30px/1.2 ui-monospace,SFMono-Regular,Menlo,monospace;letter-spacing:.08em}
table{width:100%;border-collapse:collapse}
td,th{padding:9px 0;border-bottom:1px solid var(--line);text-align:left;vertical-align:top}
th{color:var(--muted);font-weight:500;width:38%;padding-right:12px}
tr:last-child td,tr:last-child th{border-bottom:0}
.notice{border-left:4px solid var(--accent);padding:10px 14px;background:var(--panel2);border-radius:8px;margin-bottom:16px}
.notice.bad{border-color:var(--bad)}.notice.ok{border-color:var(--ok)}
code,pre{font-family:ui-monospace,Menlo,monospace}
pre{white-space:pre-wrap;word-break:break-all;background:var(--panel2);padding:12px;border-radius:8px;font-size:13px}
ol.steps{padding-left:22px}ol.steps li{margin:8px 0}
.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(250px,1fr));gap:12px}
.device{display:flex;flex-direction:column;gap:6px;text-decoration:none;color:inherit}
.device:hover{border-color:var(--accent)}
details summary{cursor:pointer;color:var(--accent);font-weight:600}
table.devices th{width:auto}
table.devices td{vertical-align:middle}
td.actions{white-space:nowrap;width:1%;text-align:right}
label.check{margin-top:10px}
.tag{font-size:12px;font-weight:600;text-transform:uppercase;letter-spacing:.06em;color:var(--muted)}
.qr{background:#fff;padding:14px;border-radius:12px;width:min(300px,100%)}
.qr svg{width:100%;height:auto;display:block}
.two{display:grid;grid-template-columns:minmax(0,1fr) minmax(0,1fr);gap:16px}
@media (max-width:720px){.two{grid-template-columns:1fr}}
"#;

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// A complete page. `refresh` reloads `url` every so many seconds, but waits
/// while someone is typing or has a section open.
pub fn page(title: &str, subtitle: &str, refresh: Option<(u32, &str)>, body: &str) -> String {
    let refresh = refresh.map_or(String::new(), |(seconds, url)| {
        format!(
            "<script>setTimeout(function r(){{var a=document.activeElement;\
             if(document.querySelector('details[open]')||(a&&/INPUT|SELECT|TEXTAREA/.test(a.tagName)))\
             {{setTimeout(r,5000);return}}location.replace({url:?})}},{ms})</script>",
            ms = seconds * 1000
        )
    });
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">{refresh}\
         <title>{title}</title><style>{STYLE}</style></head><body><div class=\"wrap\">\
         <div class=\"top\"><div class=\"logo\">O</div><div class=\"grow\"><h1>{title}</h1>\
         <div class=\"muted small\">{subtitle}</div></div></div>{body}</div></body></html>",
        title = escape(title),
        subtitle = subtitle,
    )
}

pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn ago(seconds: u64) -> String {
    match seconds {
        0..=4 => "just now".into(),
        5..=59 => format!("{seconds} s ago"),
        60..=3599 => format!("{} min ago", seconds / 60),
        3600..=86399 => format!("{} h ago", seconds / 3600),
        _ => format!("{} days ago", seconds / 86400),
    }
}
