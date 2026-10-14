use fastestvpn_gui::backend::config::{
    infer_location, parse_wireguard_conf, sanitize_id,
};
use fastestvpn_gui::backend::elevation::{validate_interface_name, ElevationManager};
use fastestvpn_gui::backend::tunnel::DEFAULT_INTERFACE_NAME;

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
    assert!(validate_interface_name(DEFAULT_INTERFACE_NAME).is_ok());
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
