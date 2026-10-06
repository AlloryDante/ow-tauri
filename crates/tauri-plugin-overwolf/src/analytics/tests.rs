//! Byte-exact request tests against stand-in vectors.
//!
//! The vectors reproduce the structure of the requests ow-electron 42.11.4
//! sends (observed) with stand-in identities: the G.2 uid vector 2
//! ("Parity Harness"), the E.4 muid of the first stand-in platform UUID, and
//! `hostLabel: "electron"`, `hostVersion: "42.11.4"`, which must reproduce
//! ow-electron's values exactly.

use serde_json::Value;

use super::*;

const UID: &str = "bijigndkghcikkfmhgkmicdkjpdehpjafgpmdhcc";
const CUID: &str = "binaioonkjpolnojeenpbmjmbfkbmffcekndbmdk";
const MUID: &str = "5bd79133-f3bf-be27-e448-a4581ab5f3cd";
const CHROME_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.7778.280 Safari/537.36";
const ELECTRON_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) ParityHarness/1.0.0 Chrome/148.0.7778.280 Electron/42.11.4 Safari/537.36";

fn reporter(label: HostLabel, cuid: &str) -> Reporter {
    let ua = compose_user_agent(CHROME_UA, "Parity Harness", "1.0.0", &label);
    Reporter {
        label,
        app_version: "1.0.0".into(),
        uid: UID.into(),
        cuid: cuid.into(),
        os: "darwin".into(),
        os_version: "25.5.0".into(),
        app_name: "Parity Harness".into(),
        muid: MUID.into(),
        muid_v2: MUID.into(),
        user_agent: ua,
        locale: "en-US".into(),
    }
}

fn electron() -> Reporter {
    reporter(HostLabel::new("electron", "42.11.4"), UID)
}

fn get_headers(ua: &str) -> Vec<(&'static str, String)> {
    vec![
        ("sec-fetch-site", "none".into()),
        ("sec-fetch-mode", "no-cors".into()),
        ("sec-fetch-dest", "empty".into()),
        ("user-agent", ua.into()),
        ("accept-encoding", "gzip, deflate, br, zstd".into()),
        ("accept-language", "en-US".into()),
        ("priority", "u=4, i".into()),
    ]
}

fn extra_base(uid: &str, cuid: &str) -> String {
    format!(
        "%7B%22app_ver%22%3A%221.0.0%22%2C%22app_id%22%3A%22{uid}%22%2C%22os%22%3A%22darwin%22%2C%22os_ver%22%3A%2225.5.0%22%2C%22app_name%22%3A%22Parity+Harness%22%2C%22app_cuid%22%3A%22{cuid}%22"
    )
}

#[test]
fn electron_label_reproduces_the_user_agent() {
    assert_eq!(electron().user_agent, ELECTRON_UA);
}

#[test]
fn counter_bytes() {
    let r = electron();
    let req = r.counter("app_first_launch", &[]);
    assert_eq!(req.method, Method::Get);
    assert_eq!(
        req.url,
        format!(
            "https://analyticsnew.overwolf.com/analytics/Counter?Name=electron_app_first_launch&MUID={MUID}&MUIDV2={MUID}&owver=42.11.4&Extra={}%7D",
            extra_base(UID, UID)
        )
    );
    assert_eq!(req.headers, get_headers(ELECTRON_UA));
    assert_eq!(req.body, None);
    assert_eq!(req.timeout, Some(REQUEST_TIMEOUT));

    let hb = r.counter(
        "app_heartbeat",
        &[("hasVisibleWindow".into(), Value::Bool(false))],
    );
    assert!(hb.url.ends_with(&format!(
        "{}%2C%22hasVisibleWindow%22%3Afalse%7D",
        extra_base(UID, UID)
    )));
}

#[test]
fn insert_stats_bytes() {
    let r = electron();
    let req = r.insert_stats(kind::FIRST_LAUNCH, None);
    assert_eq!(req.method, Method::Post);
    assert_eq!(
        req.url,
        "https://tracking.overwolf.com/tracking/InsertStats?Stats=true&owver=42_11_4"
    );
    let mut headers = vec![("content-type", "application/json".to_owned())];
    headers.extend(get_headers(ELECTRON_UA));
    assert_eq!(req.headers, headers);
    let body = format!(r#"{{"Kind":400022,"Extra":"1_0_0.{UID}.darwin.Parity Harness.{UID}"}}"#);
    assert_eq!(req.body.as_deref(), Some(body.as_bytes()));
    assert_eq!(body.len(), 135);
    let crash = r.insert_stats(kind::GUEST_CRASH, Some("killed"));
    assert_eq!(
        crash.body.unwrap(),
        format!(r#"{{"Kind":400024,"Extra":"killed.1_0_0.{UID}.darwin.Parity Harness.{UID}"}}"#)
            .into_bytes()
    );
}

#[test]
fn tauri_label_values() {
    let r = reporter(HostLabel::new("tauri", "2.12.1"), CUID);
    assert_eq!(
        r.user_agent,
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) ParityHarness/1.0.0 Chrome/148.0.7778.280 Tauri/2.12.1 Safari/537.36"
    );
    let req = r.counter("app_start", &[]);
    assert_eq!(
        req.url,
        format!(
            "https://analyticsnew.overwolf.com/analytics/Counter?Name=tauri_app_start&MUID={MUID}&MUIDV2={MUID}&owver=tauri-2.12.1&Extra={}%7D",
            extra_base(UID, CUID)
        )
    );
    let stats = r.insert_stats(kind::GUEST_ATTACH, None);
    assert!(stats.url.ends_with("owver=tauri-2_12_1"));
    assert_eq!(
        stats.body.unwrap(),
        format!(r#"{{"Kind":400025,"Extra":"1_0_0.{UID}.darwin.Parity Harness.{CUID}"}}"#)
            .into_bytes()
    );
}

#[test]
fn insert_stats_cleans_dots_and_colons() {
    let mut r = electron();
    r.app_name = "A.B: C".into();
    r.app_version = "1.2.3-beta.1".into();
    let body =
        String::from_utf8(r.insert_stats(kind::HEARTBEAT, Some("oom:x")).body.unwrap()).unwrap();
    assert_eq!(
        body,
        format!(r#"{{"Kind":400023,"Extra":"oom_x.1_2_3-beta_1.{UID}.darwin.A_B_ C.{UID}"}}"#)
    );
}

#[test]
fn cmp_eu_only_bytes() {
    let req = electron().cmp_eu_only();
    assert_eq!(
        req.url,
        "https://features.overwolf.com/experiments/cmp-eu-only"
    );
    let mut headers = vec![("cache-control", "no-cache".to_owned())];
    headers.extend(get_headers(ELECTRON_UA));
    assert_eq!(req.headers, headers);
    assert_eq!(req.timeout, None);
}

#[test]
fn sub_info_extra_order() {
    let r = electron();
    for (options, tail) in [
        (
            serde_json::json!({"providerName": "tebex", "userId": "parity-test"}),
            "%2C%22providerName%22%3A%22tebex%22%2C%22userId%22%3A%22parity-test%22%7D",
        ),
        (
            serde_json::json!({"userId": "u-1", "paymentId": "p-1"}),
            "%2C%22userId%22%3A%22u-1%22%2C%22paymentId%22%3A%22p-1%22%2C%22providerName%22%3A%22tebex%22%7D",
        ),
        (
            serde_json::json!({"providerName": "", "userId": "u"}),
            "%2C%22userId%22%3A%22u%22%2C%22providerName%22%3A%22tebex%22%7D",
        ),
    ] {
        let fields = sub_info_fields(options.as_object().unwrap());
        let req = r.counter("sub_info", &fields);
        assert!(req.url.contains("Name=electron_sub_info&"));
        assert!(
            req.url
                .ends_with(&format!("{}{tail}", extra_base(UID, UID))),
            "{}",
            req.url
        );
    }
}

#[test]
fn crash_counter_fields() {
    let req = electron().counter(
        "owadview_crashed",
        &[
            ("sessionTS".into(), Value::from(20)),
            ("reason".into(), Value::from("killed")),
        ],
    );
    assert!(
        req.url
            .ends_with("%2C%22sessionTS%22%3A20%2C%22reason%22%3A%22killed%22%7D")
    );
}

#[test]
fn window_names_match_observations() {
    for (url, name) in [
        ("tauri://localhost/index.html", "index"),
        ("http://tauri.localhost/index.html", "index"),
        (
            "https://example.com/some/path/page.php?x=1#frag",
            "page.php",
        ),
        ("https://example.com/", "example.com"),
        ("about:blank", "blank"),
        ("about:blank#second", "blank"),
        ("data:text/html,<title>d</title>", "title"),
        (
            "tauri://localhost/w/UPPER%20Case-Name_1.HTML",
            "UPPERCase-Name_1",
        ),
        (
            "tauri://localhost/w/a-very-long-file-name-over-twenty-chars.html",
            "a-very-long-file-name-over-twenty-chars",
        ),
        (
            "tauri://localhost/w/p%C3%A2g%C3%A9%20%C3%BCn%C3%AF.html",
            "pâgéünï",
        ),
        ("tauri://localhost/w/noext", "noext"),
        ("tauri://localhost/w/a.b.html", "a.b"),
        ("tauri://localhost/w/sub.dir.html?q=1", "sub.dir"),
        ("tauri://localhost/w/x.HTM", "x"),
        ("not a url", ""),
    ] {
        assert_eq!(window_analytics_name(url), name, "{url}");
    }
}

#[test]
fn user_agent_forms() {
    let label = HostLabel::new("tauri", "2.12.1");
    assert_eq!(
        compose_user_agent("", "A B", "1", &label),
        "AB/1 Tauri/2.12.1"
    );
    assert_eq!(
        compose_user_agent("X Chrome/1", "A", "2", &label),
        "X A/2 Chrome/1 Tauri/2.12.1"
    );
    assert_eq!(HostLabel::new("x_y", "1").ua_token(), "X_y/1");
}
