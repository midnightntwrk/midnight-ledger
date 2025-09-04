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

//! Derive macros for `midnight-base-crypto`.
#![deny(unreachable_pub)]
#![deny(warnings)]

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::token::Comma;
use syn::{
    ConstParam, Data, DataStruct, DeriveInput, Fields, GenericParam, Generics, Ident, Index,
    TypeParam, parse_macro_input,
};

fn generic_variants(
    generics: Generics,
) -> (
    Punctuated<GenericParam, Comma>,
    Vec<proc_macro2::TokenStream>,
    Vec<Ident>,
) {
    let generic_bounded = generics
        .params
        .clone()
        .into_iter()
        .map(|param| match param {
            GenericParam::Lifetime(p) => GenericParam::Lifetime(p),
            GenericParam::Type(TypeParam {
                ident,
                colon_token,
                bounds,
                ..
            }) => GenericParam::Type(TypeParam {
                attrs: Vec::new(),
                ident,
                colon_token,
                bounds,
                eq_token: None,
                default: None,
            }),
            GenericParam::Const(ConstParam {
                const_token,
                ident,
                colon_token,
                ty,
                ..
            }) => GenericParam::Const(ConstParam {
                attrs: Vec::new(),
                const_token,
                ident,
                colon_token,
                ty,
                eq_token: None,
                default: None,
            }),
        })
        .collect::<Punctuated<_, Comma>>();
    let generic_idents = generics
        .params
        .clone()
        .into_iter()
        .map(|param| match param {
            GenericParam::Lifetime(p) => {
                let lifetime = p.lifetime;
                quote! { #lifetime }
            }
            GenericParam::Type(p) => {
                let ident = p.ident;
                quote! { #ident }
            }
            GenericParam::Const(p) => {
                let ident = p.ident;
                quote! { #ident }
            }
        })
        .collect::<Vec<_>>();
    let type_params = generics
        .params
        .iter()
        .filter_map(|param| match param {
            GenericParam::Type(tp) => Some(tp.ident.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    (generic_bounded, generic_idents, type_params)
}

#[proc_macro_derive(FieldRepr)]
pub fn field_repr(tokens: TokenStream) -> TokenStream {
    let input = parse_macro_input!(tokens as DeriveInput);
    let name = input.ident;
    let (generic_bounded, generic_idents, type_params) = generic_variants(input.generics.clone());
    let generic_where = input.generics.where_clause;
    let where_predicates = generic_where.as_ref().map(|wh| wh.predicates.clone());

    let fields = match input.data {
        Data::Struct(DataStruct { fields, .. }) => fields,
        _ => panic!("Only structs can currently derive FieldRepr"),
    };

    let field_accessors: Vec<proc_macro2::TokenStream> = match fields.clone() {
        Fields::Unit => Vec::new(),
        Fields::Named(fields) => fields
            .named
            .into_iter()
            .map(|field| {
                let ident = field.ident.expect("named field must have a name!");
                quote!(#ident)
            })
            .collect(),
        Fields::Unnamed(fields) => fields
            .unnamed
            .into_iter()
            .enumerate()
            .map(|(i, _)| {
                let index = Index::from(i);
                quote!(#index)
            })
            .collect(),
    };

    TokenStream::from(quote! {
        impl<#generic_bounded> FieldRepr for #name<#(#generic_idents),*>
            where
               #where_predicates
               #(#type_params: FieldRepr,)*
            #generic_where
        {
            fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
                #(self.#field_accessors.field_repr(writer);)*
            }
            fn field_size(&self) -> usize {
                0 #(+ self.#field_accessors.field_size())*
            }
        }
    })
}

#[proc_macro_derive(BinaryHashRepr)]
pub fn binary_hash_repr(tokens: TokenStream) -> TokenStream {
    let input = parse_macro_input!(tokens as DeriveInput);
    let name = input.ident;
    let (generic_bounded, generic_idents, type_params) = generic_variants(input.generics.clone());
    let generic_where = input.generics.where_clause;
    let where_predicates = generic_where.as_ref().map(|wh| wh.predicates.clone());

    let fields = match input.data {
        Data::Struct(DataStruct { fields, .. }) => fields,
        _ => panic!("Only structs can currently derive BinaryHashRepr"),
    };

    let field_accessors: Vec<proc_macro2::TokenStream> = match fields.clone() {
        Fields::Unit => Vec::new(),
        Fields::Named(fields) => fields
            .named
            .into_iter()
            .map(|field| {
                let ident = field.ident.expect("named field must have a name!");
                quote!(#ident)
            })
            .collect(),
        Fields::Unnamed(fields) => fields
            .unnamed
            .into_iter()
            .enumerate()
            .map(|(i, _)| {
                let index = Index::from(i);
                quote!(#index)
            })
            .collect(),
    };
    TokenStream::from(quote! {
        impl<#generic_bounded> BinaryHashRepr for #name<#(#generic_idents),*>
            where
               #where_predicates
               #(#type_params: BinaryHashRepr,)*
            #generic_where
        {
            fn binary_repr<W: MemWrite<u8>>(&self, writer: &mut W) {
                #(self.#field_accessors.binary_repr(writer);)*
            }
            fn binary_len(&self) -> usize {
                0 #(+ self.#field_accessors.binary_len())*
            }
        }
    })
}

#[proc_macro_derive(FromFieldRepr)]
pub fn from_field_repr(tokens: TokenStream) -> TokenStream {
    let input = parse_macro_input!(tokens as DeriveInput);
    let name = input.ident;

    let fields = match input.data {
        Data::Struct(DataStruct { fields, .. }) => fields,
        _ => panic!("Only structs can currently derive FromFieldRepr"),
    };

    TokenStream::from(match fields {
        Fields::Unit => quote! {
            impl FromFieldRepr for #name {
                const FIELD_SIZE: usize = 0;
                fn from_field_repr(repr: &[Fr]) -> Option<Self> {
                    if repr.is_empty() { Some(#name) } else { None }
                }
            }
        },
        Fields::Named(fields) => {
            let field_names = fields
                .named
                .iter()
                .map(|field| field.ident.as_ref().expect("Named field must have a name!"))
                .collect::<Vec<_>>();
            let types = fields
                .named
                .iter()
                .map(|field| &field.ty)
                .collect::<Vec<_>>();
            quote! {
                impl FromFieldRepr for #name {
                    const FIELD_SIZE: usize = #(<#types as FromFieldRepr>::FIELD_SIZE)+*;
                    fn from_field_repr(mut __from_field_repr_input: &[Fr]) -> Option<Self> {
                        #(
                            let __from_field_repr_size = <#types as FromFieldRepr>::FIELD_SIZE;
                            if __from_field_repr_size > __from_field_repr_input.len() {
                                return None;
                            }
                            let #field_names = <#types as FromFieldRepr>::from_field_repr(&__from_field_repr_input[..__from_field_repr_size])?;
                            __from_field_repr_input = &__from_field_repr_input[__from_field_repr_size..];
                        )*
                        if __from_field_repr_input.len() == 0 {
                            Some(#name {
                                #(#field_names),*
                            })
                        } else {
                            None
                        }
                    }
                }
            }
        }
        Fields::Unnamed(fields) => {
            let field_names = fields
                .unnamed
                .iter()
                .enumerate()
                .map(|(i, _)| format_ident!("field_{}", i))
                .collect::<Vec<_>>();
            let types = fields
                .unnamed
                .iter()
                .map(|field| &field.ty)
                .collect::<Vec<_>>();
            quote! {
                impl FromFieldRepr for #name {
                    const FIELD_SIZE: usize = #(<#types as FromFieldRepr>::FIELD_SIZE)+*;
                    fn from_field_repr(mut repr: &[Fr]) -> Option<Self> {
                        #(
                            let size = <#types as FromFieldRepr>::FIELD_SIZE;
                            if size > repr.len() {
                                return None;
                            }
                            let #field_names = <#types>::from_field_repr(&repr[..size])?;
                            repr = &repr[size..];
                        )*
                        if repr.len() == 0 {
                            Some(#name(#(#field_names),*))
                        } else {
                            None
                        }
                    }
                }
            }
        }
    })
}
