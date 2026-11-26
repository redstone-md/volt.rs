use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{FnArg, ItemFn, LitStr, Pat, ReturnType, Type, parse2, spanned::Spanned};

pub enum Kind {
    Action,
    Loader,
    Sitemap,
    Api(&'static str),
}

struct Signature {
    item: ItemFn,
    ctx: Option<bool>,
    input: Option<Type>,
    output: Option<Type>,
}

impl Signature {
    fn parse(item: ItemFn) -> syn::Result<Self> {
        let mut ctx = None;
        let mut input = None;
        for arg in &item.sig.inputs {
            let FnArg::Typed(typed) = arg else {
                return Err(syn::Error::new(arg.span(), "methods are not supported, use a free function"));
            };
            if !matches!(*typed.pat, Pat::Ident(_) | Pat::Wild(_)) {
                return Err(syn::Error::new(typed.pat.span(), "use a plain identifier"));
            }
            if is_ctx(&typed.ty) {
                if ctx.is_some() || input.is_some() {
                    return Err(syn::Error::new(typed.span(), "`Ctx` must be the first argument"));
                }
                ctx = Some(matches!(*typed.ty, Type::Reference(_)));
            } else if input.replace((*typed.ty).clone()).is_some() {
                return Err(syn::Error::new(typed.span(), "accepts at most one input; wrap fields in a struct"));
            }
        }
        let output = match &item.sig.output {
            ReturnType::Type(_, ty) => Some((**ty).clone()),
            ReturnType::Default => None,
        };
        Ok(Self { item, ctx, input, output })
    }

    fn call(&self, input: Option<TokenStream>) -> TokenStream {
        let name = &self.item.sig.ident;
        let mut args = Vec::new();
        match self.ctx {
            Some(true) => args.push(quote!(&ctx)),
            Some(false) => args.push(quote!(ctx)),
            None => {}
        }
        args.extend(input);
        let call = quote!(#name(#(#args),*));
        if self.item.sig.asyncness.is_some() { quote!(#call.await) } else { call }
    }

    fn output(&self, what: &str) -> syn::Result<&Type> {
        self.output
            .as_ref()
            .ok_or_else(|| syn::Error::new(self.item.sig.span(), format!("{what} must return `Result<T>`")))
    }
}

fn is_ctx(ty: &Type) -> bool {
    match ty {
        Type::Path(path) => path.path.segments.last().is_some_and(|s| s.ident == "Ctx"),
        Type::Reference(reference) => is_ctx(&reference.elem),
        _ => false,
    }
}

pub fn expand(kind: Kind, args: TokenStream, item: TokenStream) -> TokenStream {
    let result = parse2::<ItemFn>(item).and_then(Signature::parse).and_then(|sig| match kind {
        Kind::Action => action(&args, sig),
        Kind::Loader => loader(args, sig),
        Kind::Sitemap => sitemap(&args, sig),
        Kind::Api(method) => api(method, args, sig),
    });
    result.unwrap_or_else(syn::Error::into_compile_error)
}

fn no_args(args: &TokenStream, what: &str) -> syn::Result<()> {
    if args.is_empty() { Ok(()) } else { Err(syn::Error::new(args.span(), format!("{what} takes no arguments"))) }
}

fn no_input(sig: &Signature, what: &str) -> syn::Result<()> {
    match &sig.input {
        Some(ty) => Err(syn::Error::new(ty.span(), format!("{what} only receives `Ctx`"))),
        None => Ok(()),
    }
}

fn path_arg(args: TokenStream, what: &str) -> syn::Result<LitStr> {
    let path: LitStr = parse2(args)
        .map_err(|_| syn::Error::new(Span::call_site(), format!("{what} expects a path: #[{what}(\"/path\")]")))?;
    if !path.value().starts_with('/') {
        return Err(syn::Error::new(path.span(), "paths must start with `/`"));
    }
    Ok(path)
}

fn action(args: &TokenStream, sig: Signature) -> syn::Result<TokenStream> {
    no_args(args, "#[action]")?;
    let item = &sig.item;
    let name = &item.sig.ident;
    let wrapper = format_ident!("__voltrs_action_{}", name);
    let name_str = name.to_string();
    let output = sig.output("an action")?;
    let (decode, input_ts) = match &sig.input {
        Some(ty) => (
            Some(quote!(let input: #ty = payload.decode()?;)),
            quote!(::core::option::Option::Some(<#ty as ::voltrs::__private::Type>::ts)),
        ),
        None => (None, quote!(::core::option::Option::None)),
    };
    let call = sig.call(sig.input.as_ref().map(|_| quote!(input)));
    Ok(quote! {
        #item

        #[doc(hidden)]
        #[allow(unused_variables)]
        fn #wrapper(
            ctx: ::voltrs::Ctx,
            payload: ::voltrs::__private::Payload,
        ) -> ::voltrs::__private::BoxFuture<::voltrs::Result<::voltrs::serde_json::Value>> {
            ::std::boxed::Box::pin(async move {
                #decode
                ::voltrs::__private::encode(#call)
            })
        }

        ::voltrs::__private::inventory::submit! {
            ::voltrs::__private::ActionDef {
                name: #name_str,
                handler: #wrapper,
                input: #input_ts,
                output: <<#output as ::voltrs::__private::Output>::Ok as ::voltrs::__private::Type>::ts,
            }
        }
    })
}

fn loader(args: TokenStream, sig: Signature) -> syn::Result<TokenStream> {
    let route = path_arg(args, "loader")?;
    no_input(&sig, "a loader")?;
    let item = &sig.item;
    let wrapper = format_ident!("__voltrs_loader_{}", item.sig.ident);
    let output = sig.output("a loader")?;
    let call = sig.call(None);
    Ok(quote! {
        #item

        #[doc(hidden)]
        #[allow(unused_variables)]
        fn #wrapper(ctx: ::voltrs::Ctx) -> ::voltrs::__private::BoxFuture<::voltrs::Result<::voltrs::serde_json::Value>> {
            ::std::boxed::Box::pin(async move { ::voltrs::__private::encode(#call) })
        }

        ::voltrs::__private::inventory::submit! {
            ::voltrs::__private::LoaderDef {
                route: #route,
                handler: #wrapper,
                output: <<#output as ::voltrs::__private::Output>::Ok as ::voltrs::__private::Type>::ts,
            }
        }
    })
}

fn sitemap(args: &TokenStream, sig: Signature) -> syn::Result<TokenStream> {
    no_args(args, "#[sitemap]")?;
    no_input(&sig, "a sitemap provider")?;
    let item = &sig.item;
    let wrapper = format_ident!("__voltrs_sitemap_{}", item.sig.ident);
    let call = sig.call(None);
    Ok(quote! {
        #item

        #[doc(hidden)]
        #[allow(unused_variables)]
        fn #wrapper(
            ctx: ::voltrs::Ctx,
        ) -> ::voltrs::__private::BoxFuture<::voltrs::Result<::std::vec::Vec<::voltrs::SitemapEntry>>> {
            ::std::boxed::Box::pin(async move { ::voltrs::__private::Output::into_result(#call) })
        }

        ::voltrs::__private::inventory::submit! {
            ::voltrs::__private::SitemapDef { handler: #wrapper }
        }
    })
}

fn api(method: &str, args: TokenStream, sig: Signature) -> syn::Result<TokenStream> {
    let path = path_arg(args, &method.to_lowercase())?;
    no_input(&sig, "an API handler")?;
    let item = &sig.item;
    let wrapper = format_ident!("__voltrs_api_{}", item.sig.ident);
    let call = sig.call(None);
    Ok(quote! {
        #item

        #[doc(hidden)]
        #[allow(unused_variables)]
        fn #wrapper(ctx: ::voltrs::Ctx) -> ::voltrs::__private::BoxFuture<::voltrs::Response> {
            ::std::boxed::Box::pin(async move { ::voltrs::__private::IntoResponse::into_response(#call) })
        }

        ::voltrs::__private::inventory::submit! {
            ::voltrs::__private::ApiDef { method: #method, path: #path, handler: #wrapper }
        }
    })
}
