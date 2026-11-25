mod suffix_data;
#[cfg(test)]
mod tests;

use suffix_data::{EDGES, LABELS, NODES, RULES_ROOT};

const SPACE: u16 = b' ' as u16;
const DOT: u16 = b'.' as u16;
const SLASH: u16 = b'/' as u16;
const COLON: u16 = b':' as u16;
const AT_SIGN: u16 = b'@' as u16;
const HASH: u16 = b'#' as u16;
const QUESTION_MARK: u16 = b'?' as u16;
const OPENING_BRACKET: u16 = b'[' as u16;
const CLOSING_BRACKET: u16 = b']' as u16;
const DASH: u16 = b'-' as u16;
const UNDERSCORE: u16 = b'_' as u16;
const PLUS: u16 = b'+' as u16;

pub(crate) fn is_linkable(url: &str, detect_ip: bool, valid_hosts: &[String]) -> bool {
    let url: Vec<u16> = url.encode_utf16().collect();
    let Some(hostname) = extract_hostname(&url, is_valid_hostname(&url)) else {
        return false;
    };
    if detect_ip && is_ip(&hostname) {
        return true;
    }
    is_valid_hostname(&hostname)
        && (has_public_suffix_rule(&hostname)
            || valid_hosts
                .iter()
                .any(|valid_host| share_same_domain_suffix(&hostname, valid_host)))
}

fn is_ascii_byte_in(code: u16, first: u8, last: u8) -> bool {
    (u16::from(first)..=u16::from(last)).contains(&code)
}

fn code_at(units: &[u16], index: usize) -> Option<u16> {
    units.get(index).copied()
}

fn to_lower_case(units: &[u16]) -> Vec<u16> {
    String::from_utf16_lossy(units)
        .to_lowercase()
        .encode_utf16()
        .collect()
}

fn index_of(haystack: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    if from > haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|position| position + from)
}

fn slice(units: &[u16], start: usize, end: usize) -> &[u16] {
    let end = end.min(units.len());
    if start >= end {
        &[]
    } else {
        &units[start..end]
    }
}

fn is_valid_scheme_code(code: u16) -> bool {
    let lower_case_code = code | 32;
    is_ascii_byte_in(lower_case_code, b'a', b'z')
        || is_ascii_byte_in(lower_case_code, b'0', b'9')
        || lower_case_code == DOT
        || lower_case_code == DASH
        || lower_case_code == PLUS
}

fn extract_hostname(url: &[u16], url_is_valid_hostname: bool) -> Option<Vec<u16>> {
    let mut start = 0;
    let mut end = url.len();
    let mut has_upper = false;

    if !url_is_valid_hostname {
        if url.starts_with(&"data:".encode_utf16().collect::<Vec<u16>>()) {
            return None;
        }

        while start < url.len() && url[start] <= SPACE {
            start += 1;
        }

        while end > start + 1 && url[end - 1] <= SPACE {
            end -= 1;
        }

        if code_at(url, start) == Some(SLASH) && code_at(url, start + 1) == Some(SLASH) {
            start += 2;
        } else if let Some(index_of_protocol) = index_of(url, &[COLON, SLASH], start) {
            if !url[start..index_of_protocol]
                .iter()
                .all(|&code| is_valid_scheme_code(code))
            {
                return None;
            }

            start = index_of_protocol + 2;
            while code_at(url, start) == Some(SLASH) {
                start += 1;
            }
        }

        let mut index_of_identifier = None;
        let mut index_of_closing_bracket = None;
        let mut index_of_port = None;
        for (index, &code) in url.iter().enumerate().take(end).skip(start) {
            match code {
                HASH | SLASH | QUESTION_MARK => {
                    end = index;
                    break;
                }
                AT_SIGN => index_of_identifier = Some(index),
                CLOSING_BRACKET => index_of_closing_bracket = Some(index),
                COLON => index_of_port = Some(index),
                _ if is_ascii_byte_in(code, b'A', b'Z') => has_upper = true,
                _ => {}
            }
        }

        if let Some(identifier) = index_of_identifier {
            if identifier > start && identifier < end {
                start = identifier + 1;
            }
        }

        if code_at(url, start) == Some(OPENING_BRACKET) {
            return index_of_closing_bracket
                .map(|closing_bracket| to_lower_case(slice(url, start + 1, closing_bracket)));
        }
        if let Some(port) = index_of_port {
            if port > start && port < end {
                end = port;
            }
        }
    }

    while end > start + 1 && url[end - 1] == DOT {
        end -= 1;
    }

    let hostname = slice(url, start, end);
    if has_upper {
        Some(to_lower_case(hostname))
    } else {
        Some(hostname.to_vec())
    }
}

fn is_valid_ascii(code: u16) -> bool {
    is_ascii_byte_in(code, b'a', b'z') || is_ascii_byte_in(code, b'0', b'9') || code > 127
}

fn is_valid_hostname(hostname: &[u16]) -> bool {
    if hostname.len() > 255 || hostname.is_empty() {
        return false;
    }

    let first_code = hostname[0];
    if !is_valid_ascii(first_code) && first_code != DOT && first_code != UNDERSCORE {
        return false;
    }

    let mut last_dot_index: isize = -1;
    let mut last_code: Option<u16> = None;
    for (index, &code) in hostname.iter().enumerate() {
        let index = index as isize;
        if code == DOT {
            if index - last_dot_index > 64
                || last_code == Some(DOT)
                || last_code == Some(DASH)
                || last_code == Some(UNDERSCORE)
            {
                return false;
            }
            last_dot_index = index;
        } else if !(is_valid_ascii(code) || code == DASH || code == UNDERSCORE) {
            return false;
        }
        last_code = Some(code);
    }

    hostname.len() as isize - last_dot_index - 1 <= 63 && last_code != Some(DASH)
}

fn is_probably_ipv4(hostname: &[u16]) -> bool {
    if hostname.len() < 7 || hostname.len() > 15 {
        return false;
    }

    let mut number_of_dots = 0;
    for &code in hostname {
        if code == DOT {
            number_of_dots += 1;
        } else if !is_ascii_byte_in(code, b'0', b'9') {
            return false;
        }
    }

    number_of_dots == 3 && hostname[0] != DOT && hostname[hostname.len() - 1] != DOT
}

fn is_probably_ipv6(hostname: &[u16]) -> bool {
    if hostname.len() < 3 {
        return false;
    }

    let start = usize::from(hostname[0] == OPENING_BRACKET);
    let mut end = hostname.len();
    if hostname[end - 1] == CLOSING_BRACKET {
        end -= 1;
    }

    if end.saturating_sub(start) > 39 {
        return false;
    }

    let mut has_colon = false;
    for &code in slice(hostname, start, end) {
        if code == COLON {
            has_colon = true;
        } else if !(is_ascii_byte_in(code, b'0', b'9')
            || is_ascii_byte_in(code, b'a', b'f')
            || is_ascii_byte_in(code, b'A', b'Z'))
        {
            return false;
        }
    }

    has_colon
}

fn is_ip(hostname: &[u16]) -> bool {
    is_probably_ipv6(hostname) || is_probably_ipv4(hostname)
}

fn edge_label(reference: u32) -> &'static str {
    let start = (reference >> 8) as usize;
    let length = (reference & 0xff) as usize;
    &LABELS[start..start + length]
}

fn find_child(node: u16, label: &str) -> Option<u16> {
    let (_, first_edge, edge_count) = NODES[usize::from(node)];
    let edges = &EDGES[usize::from(first_edge)..usize::from(first_edge) + usize::from(edge_count)];
    edges
        .binary_search_by(|&(reference, _)| edge_label(reference).cmp(label))
        .ok()
        .map(|position| edges[position].1)
}

fn trie_has_rule(labels: &[&str], root: u16) -> bool {
    let mut node = Some(root);
    let mut remaining = labels.len();
    while let Some(current) = node {
        if NODES[usize::from(current)].0 != 0 {
            return true;
        }
        if remaining == 0 {
            return false;
        }
        remaining -= 1;
        node = find_child(current, labels[remaining]).or_else(|| find_child(current, "*"));
    }
    false
}

fn has_public_suffix_rule(hostname: &[u16]) -> bool {
    let hostname = String::from_utf16_lossy(hostname);
    let labels: Vec<&str> = hostname.split('.').collect();
    trie_has_rule(&labels, RULES_ROOT)
}

fn share_same_domain_suffix(hostname: &[u16], valid_host: &str) -> bool {
    let valid_host: Vec<u16> = valid_host.encode_utf16().collect();
    hostname.ends_with(&valid_host)
        && (hostname.len() == valid_host.len()
            || hostname[hostname.len() - valid_host.len() - 1] == DOT)
}
