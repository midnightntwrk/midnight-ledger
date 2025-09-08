// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Derive macros for `midnight-serialize`.
extern crate proc_macro;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    Data, DeriveInput, Fields, GenericParam, Generics, Index, Meta, Token, parse_macro_input,
    parse_quote,
};

fn deserializable_add_trait_bounds(mut generics: Generics, phantom: &[Ident]) -> Generics {
    for param in &mut generics.params {
        if let GenericParam::Type(ref mut type_param) = *param {
            if !phantom.contains(&type_param.ident) {
                type_param.bounds.push(parse_quote!(Deserializable));
            }
        }
    }
    generics
}

fn tagged_add_trait_bounds(mut generics: Generics, phantom: &[Ident]) -> Generics {
    for param in &mut generics.params {
        if let GenericParam::Type(ref mut type_param) = *param {
            if !phantom.contains(&type_param.ident) {
                type_param.bounds.push(parse_quote!(Tagged));
            }
        }
    }
    generics
}

// Macro to implement Serializable for an object made up entirely of serializable
// objects
#[proc_macro_derive(Serializable, attributes(tag, phantom))]
pub fn derive_serializable(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = input.ident;

    let phantom_generics = input
        .attrs
        .iter()
        .find_map(|attr| match &attr.meta {
            Meta::List(l) if l.path.is_ident("phantom") => {
                let parser = Punctuated::<Ident, Token![,]>::parse_separated_nonempty;
                parser
                    .parse2(l.tokens.clone())
                    .ok()
                    .map(|punct| punct.iter().cloned().collect::<Vec<_>>())
            }
            _ => None,
        })
        .unwrap_or(vec![]);

    let generics = serializable_add_trait_bounds(input.generics.clone(), &phantom_generics);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let tag = input.attrs.iter().find_map(|attr| match &attr.meta {
        Meta::NameValue(nv) if nv.path.is_ident("tag") => Some(&nv.value),
        _ => None,
    });

    let de_generics = deserializable_add_trait_bounds(input.generics.clone(), &phantom_generics);
    let (de_impl_generics, de_ty_generics, de_where_clause) = de_generics.split_for_impl();

    let serialize = serialize(&input.data);
    let deserialize = deserialize(&input.data);
    let size = size(&input.data);

    let mut expanded = quote! {
        impl #impl_generics Serializable for #name #ty_generics #where_clause {
            fn serialize(&self, writer: &mut impl ::std::io::Write) -> Result<(), ::std::io::Error> {
                #serialize
                Ok(())
            }

            fn serialized_size(&self) -> usize {
                #size
            }
        }

        impl #de_impl_generics Deserializable for #name #de_ty_generics #de_where_clause {
            fn deserialize(reader: &mut impl ::std::io::Read, recursion_depth: u32) -> Result<Self, ::std::io::Error> {
                #deserialize
            }
        }
    };

    if let Some(tag) = tag {
        let tag_generics = tagged_add_trait_bounds(input.generics, &phantom_generics);
        let (tag_impl_generics, tag_ty_generics, tag_where_clause) = tag_generics.split_for_impl();

        let generics = tag_generics
            .params
            .iter()
            .filter_map(|param| match param {
                GenericParam::Type(ty) if !phantom_generics.contains(&ty.ident) => Some(&ty.ident),
                _ => None,
            })
            .collect::<Vec<_>>();

        let tag_expand = if generics.is_empty() {
            quote! { ::std::borrow::Cow::Borrowed(#tag) }
        } else {
            let mut fstring = String::new();
            fstring.push_str("{}(");
            for i in 0..generics.len() {
                if i > 0 {
                    fstring.push_str(",");
                }
                fstring.push_str("{}");
            }
            fstring.push_str(")");
            quote! { ::std::borrow::Cow::Owned(::std::format!(#fstring, #tag, #( <#generics as Tagged>::tag() ),*)) }
        };
        let tag_factor_expand = tag_factors(&input.data);

        expanded.extend(quote! {
            impl #tag_impl_generics Tagged for #name #tag_ty_generics #tag_where_clause {
                fn tag() -> ::std::borrow::Cow<'static, ::core::primitive::str> {
                    #tag_expand
                }
                fn tag_unique_factor() -> String {
                    #tag_factor_expand
                }
            }
        });
    }

    proc_macro::TokenStream::from(expanded)
}

fn tag_factors_fields_fmt_str(fields: &Fields) -> String {
    let nfields = fields.iter().count();
    let mut res = String::new();
    res.push('(');
    for i in 0..nfields {
        if i != 0 {
            res.push(',');
        }
        res.push_str("{}");
    }
    res.push(')');
    res
}

fn tag_factors_fields_fmt_args(fields: &Fields) -> impl Iterator<Item = TokenStream> {
    fields.iter().map(|field| &field.ty).map(|ty| {
        quote! {
            <#ty>::tag()
        }
    })
}

fn tag_factors(data: &Data) -> TokenStream {
    let fmt_str = match data {
        Data::Struct(data) => format!("({})", tag_factors_fields_fmt_str(&data.fields)),
        Data::Enum(data) => {
            let mut res = String::new();
            res.push('[');
            for (i, variant) in data.variants.iter().enumerate() {
                if i != 0 {
                    res.push(',');
                }
                res.push_str(&tag_factors_fields_fmt_str(&variant.fields));
            }
            res.push(']');
            res
        }
        Data::Union(_) => unimplemented!(),
    };
    let fmt_args: Box<dyn Iterator<Item = TokenStream>> = match data {
        Data::Struct(data) => Box::new(tag_factors_fields_fmt_args(&data.fields)),
        Data::Enum(data) => Box::new(
            data.variants
                .iter()
                .flat_map(|var| tag_factors_fields_fmt_args(&var.fields)),
        ),
        Data::Union(_) => unimplemented!(),
    };
    quote! {
        format!(#fmt_str, #(#fmt_args),*)
    }
}

fn serializable_add_trait_bounds(mut generics: Generics, phantom: &[Ident]) -> Generics {
    for param in &mut generics.params {
        if let GenericParam::Type(ref mut type_param) = *param {
            if !phantom.contains(&type_param.ident) {
                type_param.bounds.push(parse_quote!(Serializable));
            }
        }
    }
    generics
}

fn serialize_fields(fields: &Fields) -> TokenStream {
    match fields {
        Fields::Named(fields) => {
            // Expands to an expression like
            //      <A as Serializable>::serialize(a, writer)?;
            //      <B as Serializable>::serialize(b, writer)?;
            let recurse = fields.named.iter().map(|f| {
                let name = &f.ident;
                let ty = &f.ty;
                quote_spanned! {f.span()=>
                    <#ty as Serializable>::serialize(#name, writer)?;
                }
            });
            quote! {
                #(#recurse)*
            }
        }
        Fields::Unnamed(fields) => {
            let recurse = fields.unnamed.iter().enumerate().map(|(i, f)| {
                let name = Ident::new(&format!("var_{}", i), Span::call_site());
                let ty = &f.ty;
                quote_spanned! {f.span()=>
                    <#ty as Serializable>::serialize(#name, writer)?;
                }
            });
            quote! {
                #(#recurse)*
            }
        }
        Fields::Unit => TokenStream::new(),
    }
}

fn unpack_struct(fields: &Fields) -> TokenStream {
    // Expands to
    // let a = &self.a;
    // let b = &self.b;
    match fields {
        Fields::Named(fields) => {
            let recurse = fields.named.iter().map(|var| {
                let name = &var.ident;
                quote_spanned!(var.span()=>
                    let #name = &self.#name;
                )
            });
            quote! {
                #(#recurse)*
            }
        }
        Fields::Unnamed(fields) => {
            let recurse = fields.unnamed.iter().enumerate().map(|(i, var)| {
                let name = Ident::new(&format!("var_{}", i), Span::call_site());
                let index = Index::from(i);
                quote_spanned!(var.span()=>
                    let #name = &self.#index;
                )
            });
            quote! {
                #(#recurse)*
            }
        }
        Fields::Unit => TokenStream::new(),
    }
}

fn unpack_enum(fields: &Fields) -> TokenStream {
    // Expands to
    // (a, b)
    match fields {
        Fields::Named(fields) => {
            let recurse = fields.named.iter().map(|var| {
                let name = &var.ident;
                quote_spanned!(var.span()=>
                    #name,
                )
            });
            quote! {
                {#(#recurse)*}
            }
        }
        Fields::Unnamed(fields) => {
            let recurse = fields.unnamed.iter().enumerate().map(|(i, var)| {
                let name = Ident::new(&format!("var_{}", i), Span::call_site());
                quote_spanned!(var.span()=>
                    #name,
                )
            });
            quote! {
                (#(#recurse)*)
            }
        }
        Fields::Unit => TokenStream::new(),
    }
}

fn serialize(data: &Data) -> TokenStream {
    match *data {
        Data::Struct(ref data) => {
            let unpack = unpack_struct(&data.fields);
            let fields = serialize_fields(&data.fields);
            quote! {
                #unpack #fields
            }
        }
        Data::Enum(ref data) => {
            let recurse = data.variants.iter().enumerate().map(|(i, var)| {
                let fields = serialize_fields(&var.fields);
                let unpack = unpack_enum(&var.fields);
                let ty = &var.ident;
                quote_spanned! {var.span()=>
                    Self::#ty #unpack => {
                        <u8 as Serializable>::serialize(&(#i as u8), writer)?;
                        #fields
                    },
                }
            });
            quote! {
                match self {
                    #(#recurse)*
                }
            }
        }
        Data::Union(_) => TokenStream::new(),
    }
}

fn size_fields(fields: &Fields) -> TokenStream {
    match fields {
        Fields::Named(fields) => {
            // Expands to an expression like
            //      0 + <A as Serializable>::serialized_size(a)
            //      + <B as Serializable>::serialized_size(b)
            let recurse = fields.named.iter().map(|f| {
                let name = &f.ident;
                let ty = &f.ty;
                quote_spanned! {f.span()=>
                    + <#ty as Serializable>::serialized_size(#name)
                }
            });
            quote! {
                0 #(#recurse)*
            }
        }
        Fields::Unnamed(fields) => {
            let recurse = fields.unnamed.iter().enumerate().map(|(i, f)| {
                let name = Ident::new(&format!("var_{}", i), Span::call_site());
                let ty = &f.ty;
                quote_spanned! {f.span()=>
                    + <#ty as Serializable>::serialized_size(#name)
                }
            });
            quote! {
                0 #(#recurse)*
            }
        }
        Fields::Unit => quote! { 0 },
    }
}

fn size(data: &Data) -> TokenStream {
    match *data {
        Data::Struct(ref data) => {
            let unpack = unpack_struct(&data.fields);
            let fields = size_fields(&data.fields);
            quote! {
                #unpack #fields
            }
        }
        Data::Enum(ref data) => {
            let recurse = data.variants.iter().map(|var| {
                let unpack = unpack_enum(&var.fields);
                let fields = size_fields(&var.fields);
                let ty = &var.ident;
                quote_spanned! {var.span()=>
                    Self::#ty #unpack => {
                        1 + #fields
                    }
                }
            });
            quote! {
                match self {
                    #(#recurse)*
                }
            }
        }
        Data::Union(_) => unimplemented!(),
    }
}

fn deserialize_fields(fields: &Fields) -> TokenStream {
    match fields {
        Fields::Named(fields) => {
            // Expands to an expression like
            //      a: <A as Deserializable>::deserialize(reader)?,
            //      b: <B as Deserializable>::deserialize(reader)?,
            let recurse = fields.named.iter().map(|f| {
                let name = &f.ident;
                let ty = &f.ty;
                quote_spanned! {f.span()=>
                    #name: <#ty as Deserializable>::deserialize(reader, recursion_depth)?,
                }
            });
            quote! {
                {#(#recurse)*}
            }
        }
        Fields::Unnamed(fields) => {
            let recurse = fields.unnamed.iter().map(|f| {
                let ty = &f.ty;
                quote_spanned! {f.span()=>
                    <#ty as Deserializable>::deserialize(reader, recursion_depth)?,
                }
            });
            quote! {
                (#(#recurse)*)
            }
        }
        Fields::Unit => quote! {},
    }
}

fn deserialize(data: &Data) -> TokenStream {
    match *data {
        Data::Struct(ref data) => {
            let fields = deserialize_fields(&data.fields);
            quote! {
                Ok(Self #fields)
            }
        }
        Data::Enum(ref data) => {
            let recurse = data.variants.iter().enumerate().map(|(i, var)| {
                let i = i as u8;
                let fields = deserialize_fields(&var.fields);
                let name = &var.ident;
                quote_spanned! {var.span()=>
                    #i => Ok(Self::#name #fields),
                }
            });
            quote! {
                let discriminant = <u8 as Deserializable>::deserialize(reader, recursion_depth)?;
                match discriminant {
                    #(#recurse)*
                    _ => Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "unrecognised discriminant"))
                }
            }
        }
        Data::Union(_) => unimplemented!(),
    }
}
