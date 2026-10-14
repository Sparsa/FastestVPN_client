use fastestvpn_gui::backend::config::{
    infer_location, parse_wireguard_conf, sanitize_id,
};
use fastestvpn_gui::backend::elevation::{validate_interface_name, ElevationManager};
use fastestvpn_gui::backend::config::{get_fastestvpn_presets, VpnProtocol};
use fastestvpn_gui::backend::openvpn::{generate_fastestvpn_ovpn, load_ovpn_profile_from_file};
use fastestvpn_gui::backend::tunnel::{
    TunnelController, TunnelError, DEFAULT_OPENVPN_INTERFACE, DEFAULT_WG_INTERFACE,
};

#[test]
fn test_wireguard_parser_valid_config() {
    let conf = r#"
# FastestVPN Frankfurt Node
[Interface]
PrivateKey = aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa=
Address = 10.14.0.2/32, fd86:ea04:1115::2/64
DNS = 1.1.1.1, 1.0.0.1
MTU = 1420

[Peer]
PublicKey = bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb=
Endpoint = de-01.jumptoserver.com:51820
AllowedIPs = 0.0.0.0/0, ::/0
PersistentKeepalive = 25
"#;

    let parsed = parse_wireguard_conf(conf).expect("Must parse valid config");
    assert_eq!(parsed.interface.private_key, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa=");
    assert_eq!(parsed.interface.address, vec!["10.14.0.2/32", "fd86:ea04:1115::2/64"]);
    assert_eq!(parsed.interface.dns, vec!["1.1.1.1", "1.0.0.1"]);
    assert_eq!(parsed.interface.mtu, Some(1420));
    assert_eq!(parsed.peer.public_key, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb=");
    assert_eq!(parsed.peer.endpoint, "de-01.jumptoserver.com:51820");
    assert_eq!(parsed.peer.allowed_ips, vec!["0.0.0.0/0", "::/0"]);
    assert_eq!(parsed.peer.persistent_keepalive, Some(25));

    // Test serialization
    let serialized = parsed.to_conf_string();
    assert!(serialized.contains("[Interface]"));
    assert!(serialized.contains("PrivateKey = aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa="));
    assert!(serialized.contains("Endpoint = de-01.jumptoserver.com:51820"));
}

#[test]
fn test_wireguard_parser_missing_interface_field() {
    let invalid_conf = r#"
[Interface]
# Missing PrivateKey
Address = 10.14.0.2/32

[Peer]
PublicKey = bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb=
Endpoint = de-01.jumptoserver.com:51820
"#;

    let res = parse_wireguard_conf(invalid_conf);
    assert!(res.is_err(), "Should fail when PrivateKey is missing");
}

#[test]
fn test_elevation_safety_interface_validation() {
    // Valid interfaces (IFNAMSIZ <= 15)
    assert!(validate_interface_name(DEFAULT_WG_INTERFACE).is_ok());
    assert!(validate_interface_name(DEFAULT_OPENVPN_INTERFACE).is_ok());
    assert!(validate_interface_name("wg0").is_ok());
    assert!(validate_interface_name("fastest-vpn").is_ok());
    assert!(validate_interface_name("fvpn_de").is_ok());

    // Injection & path traversal attempts must be rejected
    assert!(validate_interface_name("; rm -rf /").is_err());
    assert!(validate_interface_name("wg0 && ls").is_err());
    assert!(validate_interface_name("$(id)").is_err());
    assert!(validate_interface_name("`reboot`").is_err());
    assert!(validate_interface_name("../etc/passwd").is_err());
    assert!(validate_interface_name("interface_name_that_exceeds_15_chars").is_err());
    assert!(validate_interface_name("").is_err());
}

#[test]
fn test_location_heuristics() {
    let (country, code, city) = infer_location("germany-frankfurt-tcp", "de-01.jumptoserver.com:51820");
    assert_eq!(country, "Germany");
    assert_eq!(code, "DE");
    assert_eq!(city, "Frankfurt");

    let (country, code, city) = infer_location("FastestVPN US Dallas", "us-dl.jumptoserver.com:51820");
    assert_eq!(country, "United States");
    assert_eq!(code, "US");
    assert_eq!(city, "Dallas");

    let (country, code, city) = infer_location("tokyo", "jp-01.jumptoserver.com:51820");
    assert_eq!(country, "Japan");
    assert_eq!(code, "JP");
    assert_eq!(city, "Tokyo");
}

#[test]
fn test_sanitize_id() {
    assert_eq!(sanitize_id("Germany - Frankfurt"), "germany___frankfurt");
    assert_eq!(sanitize_id("US-Dallas!@#"), "us_dallas");
}

#[test]
fn test_polkit_policy_generation() {
    let policy = ElevationManager::generate_polkit_policy();
    assert!(policy.contains("com.fastestvpn.wgquick"));
    assert!(policy.contains("/usr/bin/wg-quick"));
    assert!(policy.contains("Authentication is required to start or stop FastestVPN WireGuard tunnels"));
}


#[test]
fn test_ovpn_generation() {
    let tcp = generate_fastestvpn_ovpn("de-01.jumptoserver.com", VpnProtocol::OpenVpnTcp);
    assert!(tcp.contains("remote de-01.jumptoserver.com 4443"));
    assert!(tcp.contains("proto tcp"));
    assert!(tcp.contains("<ca>") && tcp.contains("</tls-auth>"));
    assert!(tcp.contains("data-ciphers-fallback AES-256-CBC"));

    let udp = generate_fastestvpn_ovpn("nl-01.jumptoserver.com", VpnProtocol::OpenVpnUdp);
    assert!(udp.contains("proto udp"));
}

#[test]
fn test_ovpn_profile_id_stable_across_reload() {
    let dir = std::env::temp_dir().join(format!("fvpn-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let original = dir.join("germany-frankfurt-tcp.ovpn");
    std::fs::write(
        &original,
        generate_fastestvpn_ovpn("de-01.jumptoserver.com", VpnProtocol::OpenVpnTcp),
    )
    .unwrap();
    let imported = load_ovpn_profile_from_file(&original).unwrap();
    assert_eq!(imported.protocol, VpnProtocol::OpenVpnTcp);
    assert_eq!(imported.display_endpoint(), "de-01.jumptoserver.com:4443");

    // ProfileStore stores imports as `<id>.ovpn`; reloading must yield the same id.
    let stored = dir.join(format!("{}.ovpn", imported.id));
    std::fs::copy(&original, &stored).unwrap();
    let reloaded = load_ovpn_profile_from_file(&stored).unwrap();
    assert_eq!(reloaded.id, imported.id);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_wireguard_refuses_placeholder_credentials() {
    // Presets without a user key must not reach pkexec / wg-quick.
    let controller = TunnelController::default();
    let preset = get_fastestvpn_presets("", "").remove(0);
    match controller.start_wireguard_tunnel(&preset) {
        Err(TunnelError::InvalidProfile(_)) => {}
        other => panic!("expected InvalidProfile, got {other:?}"),
    }
}
