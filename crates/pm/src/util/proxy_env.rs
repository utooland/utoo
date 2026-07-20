use colored::Colorize;
use std::ffi::OsStr;
use std::sync::Once;

const PROXY_ENV_VARS: [&str; 6] = [
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
];

static PRINT_PROXY_HINT_ONCE: Once = Once::new();

fn collect_proxy_env_vars_from<I, V>(vars: I) -> Vec<&'static str>
where
    I: IntoIterator<Item = (&'static str, Option<V>)>,
    V: AsRef<OsStr>,
{
    vars.into_iter()
        .filter_map(|(key, value)| value.filter(|v| !v.as_ref().is_empty()).map(|_| key))
        .collect()
}

pub fn print_proxy_env_hint_once() {
    if crate::util::invocation::json() || crate::util::invocation::quiet() {
        return;
    }

    PRINT_PROXY_HINT_ONCE.call_once(|| {
        let detected =
            collect_proxy_env_vars_from(PROXY_ENV_VARS.map(|key| (key, std::env::var_os(key))));
        if !detected.is_empty() {
            eprintln!(
                "{} {} {}",
                "!".blue().bold(),
                "proxy env:".blue().bold(),
                detected.join(", ").cyan()
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_proxy_env_vars_ignores_unset_and_empty_values() {
        let vars = [
            ("http_proxy", Some("")),
            ("https_proxy", None),
            ("all_proxy", Some("socks5://127.0.0.1:7891")),
        ];

        assert_eq!(collect_proxy_env_vars_from(vars), vec!["all_proxy"]);
    }

    #[test]
    fn collect_proxy_env_vars_reports_supported_proxy_keys_in_stable_order() {
        let vars = [
            ("http_proxy", Some("http://127.0.0.1:7890")),
            ("https_proxy", None),
            ("all_proxy", Some("socks5://127.0.0.1:7891")),
            ("HTTP_PROXY", None),
            ("HTTPS_PROXY", Some("http://127.0.0.1:7890")),
            ("ALL_PROXY", None),
        ];

        assert_eq!(
            collect_proxy_env_vars_from(vars),
            vec!["http_proxy", "all_proxy", "HTTPS_PROXY"]
        );
    }
}
