use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse_macro_input, Data, DeriveInput, Fields,
};

#[proc_macro_derive(I18nError, attributes(i18n))]
pub fn derive_i18n_error(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let enum_name = &input.ident;

    let Data::Enum(data_enum) = input.data else {
        panic!("I18nError can only be derived for enums");
    };

    let mut key_arms = Vec::new();
    let mut param_arms = Vec::new();

    for variant in data_enum.variants {
        let v_ident = &variant.ident;

        let attr = variant
            .attrs
            .iter()
            .find(|a| a.path().is_ident("i18n"))
            .expect("Missing #[i18n(\"...\")] attribute");

        let key: syn::LitStr = attr.parse_args().unwrap();

        match &variant.fields {
            Fields::Unit => {
                key_arms.push(quote! {
                    Self::#v_ident => #key
                });
                param_arms.push(quote! {
                    Self::#v_ident => None
                });
            }

            Fields::Unnamed(fields) => {
                let vars: Vec<_> = (0..fields.unnamed.len())
                    .map(|i| syn::Ident::new(&format!("v{i}"), proc_macro2::Span::call_site()))
                    .collect();

                key_arms.push(quote! {
                    Self::#v_ident( .. ) => #key
                });

                param_arms.push(quote! {
                    Self::#v_ident(#(#vars),*) => {
                        Some(I18nParam::Tuple(vec![#(Box::new(#vars.clone()) as Box<dyn std::any::Any>),*]))
                    }
                });
            }

            Fields::Named(fields) => {
                let names: Vec<_> = fields.named.iter().map(|f| f.ident.as_ref().unwrap()).collect();

                key_arms.push(quote! {
                    Self::#v_ident { .. } => #key
                });

                param_arms.push(quote! {
                    Self::#v_ident { #(#names),* } => {
                        Some(I18nParam::Struct(vec![
                            #( (stringify!(#names), Box::new(#names.clone()) as Box<dyn std::any::Any>) ),*
                        ]))
                    }
                });
            }
        }
    }

    quote! {
        impl #enum_name {
            pub fn get_key(&self) -> &'static str {
                match self {
                    #(#key_arms),*
                }
            }

            pub fn get_param(&self) -> Option<I18nParam> {
                match self {
                    #(#param_arms),*
                }
            }
        }
    }
        .into()
}