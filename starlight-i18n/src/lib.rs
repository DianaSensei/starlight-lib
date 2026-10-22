use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, parse_macro_input};

/// Derives `get_i18n_code()` returning the `#[i18n_code("...")]` key of each enum variant.
#[proc_macro_derive(I18nCode, attributes(i18n_code))]
pub fn derive_i18n_key(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let enum_name = &input.ident;
    let Data::Enum(data_enum) = &input.data else {
        return Err(syn::Error::new_spanned(
            input,
            "I18nCode can only be derived for enums",
        ));
    };

    let match_arms = data_enum
        .variants
        .iter()
        .map(|variant| {
            let ident = &variant.ident;
            let attribute = variant
                .attrs
                .iter()
                .find(|attribute| attribute.path().is_ident("i18n_code"))
                .ok_or_else(|| {
                    syn::Error::new_spanned(variant, "missing #[i18n_code(\"...\")] attribute")
                })?;
            let key: LitStr = attribute.parse_args()?;
            let pattern = match variant.fields {
                Fields::Named(_) => quote! { Self::#ident { .. } },
                Fields::Unnamed(_) => quote! { Self::#ident ( .. ) },
                Fields::Unit => quote! { Self::#ident },
            };
            Ok(quote! { #pattern => #key })
        })
        .collect::<syn::Result<Vec<_>>>()?;

    Ok(quote! {
        impl #enum_name {
            pub fn get_i18n_code(&self) -> &'static str {
                match self {
                    #(#match_arms),*
                }
            }
        }
    })
}
