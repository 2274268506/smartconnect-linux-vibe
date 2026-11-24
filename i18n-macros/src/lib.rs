//! Preserve Rust's format checking and named captures while making values available to catalogs.
use proc_macro::TokenStream;
use quote::quote;
use syn::{
    LitStr, Token,
    parse::{Parse, ParseStream},
};
struct Input {
    english: LitStr,
    chinese: LitStr,
    rest: proc_macro2::TokenStream,
}
impl Parse for Input {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let english = input.parse()?;
        input.parse::<Token![,]>()?;
        let chinese = input.parse()?;
        let rest = input.parse()?;
        Ok(Self {
            english,
            chinese,
            rest,
        })
    }
}
fn marked(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let (mut i, mut index) = (0, 0);
    while i < chars.len() {
        if chars[i] == '{' && chars.get(i + 1) == Some(&'{') {
            out.push_str("{{");
            i += 2;
            continue;
        }
        if chars[i] == '{' {
            let mut end = i + 1;
            while end < chars.len() && chars[end] != '}' {
                end += 1;
            }
            if end < chars.len() {
                out.push('\u{e000}');
                out.push_str(&index.to_string());
                out.push('\u{e001}');
                out.extend(chars[i..=end].iter());
                out.push('\u{e002}');
                out.push_str(&index.to_string());
                out.push('\u{e003}');
                index += 1;
                i = end + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}
#[proc_macro]
pub fn tr_format(input: TokenStream) -> TokenStream {
    let Input {
        english,
        chinese,
        rest,
    } = syn::parse_macro_input!(input as Input);
    let encoded = LitStr::new(&marked(&english.value()), english.span());
    quote!({
        if crate::i18n::is_chinese(){format!(#chinese #rest)}
        else{crate::i18n::format_message(#english,format!(#encoded #rest))}
    })
    .into()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn marks_only_real_placeholders_and_preserves_rust_format_specifiers() {
        assert_eq!(
            marked("{{literal}} {name} {:.1}"),
            "{{literal}} \u{e000}0\u{e001}{name}\u{e002}0\u{e003} \u{e000}1\u{e001}{:.1}\u{e002}1\u{e003}"
        );
    }
}
