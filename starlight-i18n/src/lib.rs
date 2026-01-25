use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Data, Fields};

#[proc_macro_derive(I18nCode, attributes(i18n_code))]
pub fn derive_i18n_key(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let enum_name = &input.ident;

    let Data::Enum(data_enum) = input.data else {
        panic!("I18nCode can only be derived for enums");
    };

    let mut match_arms = Vec::new();

    for variant in data_enum.variants {
        let ident = &variant.ident;

        let attr = variant
            .attrs
            .iter()
            .find(|a| a.path().is_ident("i18n_code"))
            .expect("Missing #[i18n_code(\"...\")] attribute");

        let key: syn::LitStr = attr
            .parse_args()
            .expect("i18n attribute must be a string literal");

        let arm = match variant.fields {
            Fields::Named(_) => quote! { Self::#ident { .. } => #key },
            Fields::Unnamed(_) => quote! { Self::#ident ( .. ) => #key },
            Fields::Unit => quote! { Self::#ident => #key },
        };
        match_arms.push(arm);
    }

    quote! {
        impl #enum_name {
            pub fn get_i18n_code(&self) -> &'static str {
                match self {
                    #(#match_arms),*
                }
            }
        }
    }
        .into()
}