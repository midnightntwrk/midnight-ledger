use proc_macro::TokenStream;
use proc_macro2::Literal;
use quote::quote;

const VERSION: &str = include_str!("../version");

#[proc_macro]
pub fn version(_input: TokenStream) -> TokenStream {
    let v = VERSION.trim_end_matches('\n');
    let lit = Literal::string(v);
    quote!(#lit).into()
}
