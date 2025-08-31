extern crate proc_macro;
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{ItemFn, LitStr, parse_macro_input};

/// #[rpc("name")]
#[proc_macro_attribute]
pub fn rpc(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let name_lit = parse_macro_input!(attr as LitStr);
    let rpc_name = name_lit.value();

    let fn_ident = &input_fn.sig.ident;
    let struct_ident = format_ident!("__Rpc_{}", fn_ident);
    let static_ident = format_ident!("RPC_{}", fn_ident.to_string().to_uppercase());

    let expanded = quote! {
        #input_fn

        #[allow(non_camel_case_types)]
        pub struct #struct_ident;

        impl ::zetax_types::handlers::Rpc for #struct_ident {
            fn name(&self) -> &'static str { #rpc_name }
            fn call(&self,
                    ctx: ::zetax_types::handlers::RpcContext,
                    input: ::std::vec::Vec<u8>)
                -> ::zetax_types::handlers::RpcFut
            {
                ::zetax_types::handlers::RpcBox::boxed(async move {
                    #fn_ident(ctx, input).await
                })
            }
        }

        #[allow(non_upper_case_globals)]
        pub static #static_ident: #struct_ident = #struct_ident;
    };

    TokenStream::from(expanded)
}

/// #[rstream("name")]
#[proc_macro_attribute]
pub fn rstream(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let name_lit = parse_macro_input!(attr as LitStr);
    let rpc_name = name_lit.value();

    let fn_ident = &input_fn.sig.ident;
    let struct_ident = format_ident!("__RpcStream_{}", fn_ident);
    let static_ident = format_ident!("RPC_STREAM_{}", fn_ident.to_string().to_uppercase());

    let expanded = quote! {
        #input_fn

        #[allow(non_camel_case_types)]
        pub struct #struct_ident;

        impl ::zetax_types::handlers::RpcStream for #struct_ident {
            fn name(&self) -> &'static str { #rpc_name }
            fn call_stream(
                &self,
                ctx: ::zetax_types::handlers::RpcContext,
                input: ::std::vec::Vec<u8>
            ) -> ::zetax_types::handlers::RpcStreamFut {
                ::zetax_types::handlers::RpcStreamBox::boxed(async move {
                    #fn_ident(ctx, input).await
                })
            }
        }

        #[allow(non_upper_case_globals)]
        pub static #static_ident: #struct_ident = #struct_ident;
    };

    TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn capnp(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let name_lit = parse_macro_input!(attr as LitStr);
    let rpc_name = name_lit.value();

    let fn_ident = &input_fn.sig.ident;
    let struct_ident = format_ident!("__CapnpRpc_{}", fn_ident);
    let static_ident = format_ident!("CAPNP_RPC_{}", fn_ident.to_string().to_uppercase());

    let expanded = quote! {
        #input_fn

        #[allow(non_camel_case_types)]
        pub struct #struct_ident;

        impl ::zetax_types::handlers::Rpc for #struct_ident {
            fn name(&self) -> &'static str { #rpc_name }

            fn call(
                &self,
                ctx: ::zetax_types::handlers::RpcContext,
                input: ::std::vec::Vec<u8>
            ) -> ::zetax_types::handlers::RpcFut {
                ::zetax_types::handlers::RpcBox::boxed(async move {
                    let message_reader = ::capnp::serialize::read_message(
                        &mut &input[..],
                        ::capnp::message::ReaderOptions::new()
                    ).map_err(|e| {
                        Box::new(::zetax_types::errors::RpcError::Deserialization(e.to_string()))
                            as Box<dyn ::std::error::Error + Send + Sync>
                    })?;

                    let response_message = #fn_ident(ctx, message_reader)
                        .await
                        .map_err(|e| Box::new(e) as Box<dyn ::std::error::Error + Send + Sync>)?;

                    let mut buf: ::std::vec::Vec<u8> = ::std::vec::Vec::new();
                    ::capnp::serialize::write_message(&mut buf, &response_message).map_err(|e| {
                        Box::new(::zetax_types::errors::RpcError::Serialization(e.to_string()))
                            as Box<dyn ::std::error::Error + Send + Sync>
                    })?;

                    Ok(buf)
                })
            }
        }

        #[allow(non_upper_case_globals)]
        pub static #static_ident: #struct_ident = #struct_ident;
    };

    TokenStream::from(expanded)
}
