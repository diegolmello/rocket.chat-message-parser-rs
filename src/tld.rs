#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TldOptions {
    pub detect_ip: bool,
    pub allow_private_domains: bool,
    pub valid_hosts: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TldResult {
    pub hostname: Option<String>,
    pub domain: Option<String>,
    pub public_suffix: Option<String>,
    pub is_icann: Option<bool>,
    pub is_private: Option<bool>,
    pub is_ip: Option<bool>,
}

pub fn parse(_url: &str, _options: &TldOptions) -> TldResult {
    TldResult::default()
}
