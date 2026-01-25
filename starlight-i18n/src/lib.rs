use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Data};

#[proc_macro_derive(I18nKey, attributes(i18n))]
pub fn derive_i18n_key(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let enum_name = &input.ident;

    let Data::Enum(data_enum) = input.data else {
        panic!("I18nKey can only be derived for enums");
    };

    let mut match_arms = Vec::new();

    for variant in data_enum.variants {
        let ident = &variant.ident;

        let attr = variant
            .attrs
            .iter()
            .find(|a| a.path().is_ident("i18n"))
            .expect("Missing #[i18n(\"...\")] attribute");

        let key: syn::LitStr = attr
            .parse_args()
            .expect("i18n attribute must be a string literal");

        match_arms.push(quote! {
            Self::#ident { .. } => #key,
            Self::#ident ( .. ) => #key,
            Self::#ident => #key,
        });
    }

    quote! {
        impl #enum_name {
            pub fn get_key(&self) -> &'static str {
                match self {
                    #(#match_arms)*
                }
            }
        }
    }
        .into()
}