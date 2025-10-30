use proc_macro::TokenStream;
use proc_macro2::Literal;
use quote::quote;

#[proc_macro]
pub fn version(_input: TokenStream) -> TokenStream {
    let v = include_str!("../version").trim_end_matches('\n');
    let lit = Literal::string(v);
    quote!(#lit).into()
}
