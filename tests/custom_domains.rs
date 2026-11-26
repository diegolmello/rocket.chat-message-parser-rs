use message_parser::ast::{paragraph, plain};
use message_parser::{parse, Options};

#[test]
fn an_empty_custom_domain_links_nothing() {
    for custom_domains in [vec![""], vec!["", "local"]] {
        let options = Options {
            custom_domains: Some(custom_domains.into_iter().map(String::from).collect()),
            ..Options::default()
        };

        assert_eq!(
            parse("b.m@.:]", &options),
            [paragraph(vec![plain("b.m@.:]")])]
        );
    }
}
