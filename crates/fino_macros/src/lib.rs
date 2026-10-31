//! RSX authoring macros for Fino.
//! 面向 Fino 的 RSX 作者宏。
//!
//! This is the counter-stage subset only: one root `<button>`, static text,
//! dynamic text, `on:click`, and dynamic `aria-label`.
//! 这里只实现计数器阶段子集：一个 `<button>` 根节点、静态文本、动态文本、
//! `on:click` 与动态 `aria-label`。

use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::{Ident, Span, TokenStream};
use proc_macro2_diagnostics::Level;
use quote::{format_ident, quote};
use rstml::{
    Parser, ParserConfig,
    node::{Node, NodeAttribute, NodeBlock, NodeElement, NodeText},
};
use syn::{Expr, ExprClosure, ExprLit, Lit, LitStr, Stmt, spanned::Spanned, visit::Visit};

/// Builds one Fino view from the counter-stage RSX subset.
/// 从计数器阶段 RSX 子集构建一个 Fino 视图。
///
/// The expansion requires `runtime` and `owner` in scope and returns a
/// [`fino::view::ViewOutput`](https://docs.rs/fino).
/// 展开代码要求作用域内存在 `runtime` 与 `owner`，并返回
/// [`fino::view::ViewOutput`](https://docs.rs/fino)。
#[proc_macro]
pub fn view(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    expand(input.into()).into()
}

fn expand(input: TokenStream) -> TokenStream {
    let parser = Parser::new(ParserConfig::default());
    let parsed = parser.parse_recoverable(input);
    let (nodes, diagnostics) = parsed.split();

    let mut errors = None;
    for diagnostic in diagnostics {
        if diagnostic.level() == Level::Error {
            combine(&mut errors, diagnostic.into());
        }
    }

    let Some(nodes) = nodes else {
        return errors
            .unwrap_or_else(|| syn::Error::new(Span::call_site(), "view! parsing failed"))
            .to_compile_error();
    };

    let mut builder = Builder::default();
    if let Err(error) = builder.expand(nodes) {
        combine(&mut errors, error);
    }

    match errors {
        Some(error) => error.to_compile_error(),
        None => builder.finish(),
    }
}

fn combine(errors: &mut Option<syn::Error>, error: syn::Error) {
    match errors {
        Some(errors) => errors.combine(error),
        None => *errors = Some(error),
    }
}

#[derive(Default)]
struct Builder {
    template: Vec<TokenStream>,
    bindings: Vec<Binding>,
    closures: Vec<Closure>,
    text_slots: Vec<usize>,
    next_node: usize,
}

struct Closure {
    expression: Expr,
    captured: BTreeSet<String>,
}

enum Binding {
    Text {
        node: usize,
        name: LitStr,
        closure: usize,
    },
    Attribute {
        node: usize,
        attribute: Ident,
        closure: usize,
    },
    Event {
        node: usize,
        closure: usize,
    },
}

impl Builder {
    fn expand(&mut self, nodes: Vec<Node>) -> syn::Result<()> {
        let roots: Vec<&Node> = nodes.iter().filter(|node| !ignored_at_root(node)).collect();
        let root = match roots.as_slice() {
            [root] => *root,
            [] => {
                return Err(syn::Error::new(
                    Span::call_site(),
                    "view! requires one root <button> element",
                ));
            }
            [_, second, ..] => {
                return Err(syn::Error::new_spanned(
                    second,
                    "view! supports exactly one root element",
                ));
            }
        };

        let Node::Element(element) = root else {
            return Err(syn::Error::new_spanned(
                root,
                "view! requires one root <button> element",
            ));
        };

        self.element(element, None)?;
        Ok(())
    }

    fn next_node(&mut self) -> usize {
        let node = self.next_node;
        self.next_node += 1;
        node
    }

    fn finish(self) -> TokenStream {
        let Builder {
            template,
            bindings,
            closures,
            text_slots,
            next_node: _,
        } = self;
        let mut shared = BTreeMap::new();
        for closure in &closures {
            for identifier in &closure.captured {
                *shared.entry(identifier.clone()).or_insert(0) += 1;
            }
        }

        let bindings = bindings.iter().map(|binding| {
            let closure = match binding {
                Binding::Text { closure, .. }
                | Binding::Attribute { closure, .. }
                | Binding::Event { closure, .. } => *closure,
            };
            let expression = closure_tokens(&closures[closure], &shared);
            match binding {
                Binding::Text {
                    node,
                    name,
                    closure: _,
                } => quote! {
                    ::fino::view::TextBinding::new(#name, #expression)
                        .mount(runtime, owner, &__fino_nodes[#node]);
                },
                Binding::Attribute {
                    node,
                    attribute,
                    closure: _,
                } => quote! {
                    ::fino::view::AttributeBinding::new(
                        ::fino::dom_api::Attribute::#attribute,
                        #expression,
                    )
                    .mount(runtime, owner, &__fino_nodes[#node]);
                },
                Binding::Event { node, closure: _ } => quote! {
                    ::fino::view::EventBinding::click(#expression)
                        .mount(runtime, owner, &__fino_nodes[#node]);
                },
            }
        });
        let slots = text_slots
            .iter()
            .map(|slot| quote! { __fino_nodes[#slot].clone() });

        quote! {{
            let mut __fino_template = ::fino::view::Template::new();
            #(#template)*
            let __fino_nodes = __fino_template.mount(runtime);
            #(#bindings)*
            ::fino::view::ViewOutput::new(
                __fino_nodes[0].clone(),
                ::std::vec![#(#slots),*],
            )
        }}
    }

    fn element(
        &mut self,
        element: &NodeElement<rstml::Infallible>,
        parent: Option<usize>,
    ) -> syn::Result<usize> {
        let name = element.name().to_string();
        let tag = match name.as_str() {
            "button" => "Button",
            _ => {
                return Err(syn::Error::new_spanned(
                    element.name(),
                    format!("unsupported tag `{name}`; view! supports only <button>"),
                ));
            }
        };
        let tag: Ident = Ident::new(tag, element.name().span());
        let node = self.next_node();
        let node_ident = format_ident!("__fino_node_{node}");
        let parent = match parent {
            Some(parent) => {
                let parent_ident = format_ident!("__fino_node_{parent}");
                quote! { Some(#parent_ident) }
            }
            None => quote! { None },
        };
        self.template.push(quote! {
            let #node_ident = __fino_template.element(
                #parent,
                ::fino::dom_api::Tag::#tag,
            );
        });

        let mut errors = None;
        for attribute in element.attributes() {
            if let Err(error) = self.attribute(attribute, node) {
                combine(&mut errors, error);
            }
        }
        for child in &element.children {
            if let Err(error) = self.child(child, node) {
                combine(&mut errors, error);
            }
        }

        match errors {
            Some(error) => Err(error),
            None => Ok(node),
        }
    }

    fn attribute(&mut self, attribute: &NodeAttribute, node: usize) -> syn::Result<()> {
        let NodeAttribute::Attribute(attribute) = attribute else {
            return Err(syn::Error::new_spanned(
                attribute,
                "dynamic attribute keys are not supported by view!",
            ));
        };
        let key = attribute.key.to_string();
        if let Some(event) = key.strip_prefix("on:") {
            if event != "click" {
                return Err(syn::Error::new_spanned(
                    &attribute.key,
                    format!("unsupported event `{key}`; view! supports only on:click"),
                ));
            }
            let value = attribute.value().ok_or_else(|| {
                syn::Error::new_spanned(&attribute.key, "on:click requires a closure value")
            })?;
            let closure = self.closure(value, ClosureKind::Event)?;
            self.bindings.push(Binding::Event { node, closure });
            return Ok(());
        }

        let attribute_name = match key.as_str() {
            "aria-label" => "AriaLabel",
            _ => {
                return Err(syn::Error::new_spanned(
                    &attribute.key,
                    format!("unsupported attribute `{key}`; view! supports only aria-label"),
                ));
            }
        };
        let attribute_ident = Ident::new(attribute_name, attribute.key.span());
        let value = attribute.value().ok_or_else(|| {
            syn::Error::new_spanned(&attribute.key, format!("{key} requires a value"))
        })?;

        if let Some(literal) = string_literal(value) {
            let node_variable = Ident::new(&format!("__fino_node_{node}"), literal.span());
            self.template.push(quote! {
                __fino_template.static_attribute(
                    #node_variable,
                    ::fino::dom_api::Attribute::#attribute_ident,
                    #literal,
                );
            });
            return Ok(());
        }

        let closure = self.closure(value, ClosureKind::Attribute)?;
        self.bindings.push(Binding::Attribute {
            node,
            attribute: attribute_ident,
            closure,
        });
        Ok(())
    }

    fn child(&mut self, child: &Node, parent: usize) -> syn::Result<()> {
        match child {
            Node::Text(text) => self.text(text, parent),
            Node::Block(block) => self.dynamic_text(block, parent),
            Node::Element(element) => {
                self.element(element, Some(parent))?;
                Ok(())
            }
            Node::Comment(_) => Ok(()),
            Node::RawText(raw) => {
                if raw.to_token_stream_string().trim().is_empty() {
                    Ok(())
                } else {
                    Err(syn::Error::new_spanned(
                        raw,
                        "unquoted text is not supported; quote static text with \"...\"",
                    ))
                }
            }
            Node::Fragment(fragment) => Err(syn::Error::new_spanned(
                fragment,
                "fragments are not supported by the counter-stage view!",
            )),
            Node::Doctype(doctype) => Err(syn::Error::new_spanned(
                doctype,
                "doctype is not supported inside view!",
            )),
            Node::Custom(custom) => Err(syn::Error::new_spanned(
                custom,
                "custom nodes are not supported by view!",
            )),
        }
    }

    fn text(&mut self, text: &NodeText, parent: usize) -> syn::Result<()> {
        let value = text.value.value();
        if value.is_empty() {
            return Ok(());
        }
        let literal = &text.value;
        let node = self.next_node();
        let node_ident = format_ident!("__fino_node_{node}");
        let parent_ident = format_ident!("__fino_node_{parent}");
        self.template.push(quote! {
            let #node_ident = __fino_template.text(#parent_ident, #literal);
        });
        Ok(())
    }

    fn dynamic_text(&mut self, block: &NodeBlock, parent: usize) -> syn::Result<()> {
        let expression = block_expression(block)?;
        let closure = self.closure(expression, ClosureKind::Text)?;
        let node = self.next_node();
        let node_ident = format_ident!("__fino_node_{node}");
        let parent_ident = format_ident!("__fino_node_{parent}");
        self.template.push(quote! {
            let #node_ident = __fino_template.text_slot(#parent_ident);
        });
        self.text_slots.push(node);
        let name = LitStr::new(&format!("view/text/{node}"), expression.span());
        self.bindings.push(Binding::Text {
            node,
            name,
            closure,
        });
        Ok(())
    }

    fn closure(&mut self, expression: &Expr, kind: ClosureKind) -> syn::Result<usize> {
        let expression = ungroup(expression);
        let Expr::Closure(closure) = expression else {
            return Err(syn::Error::new_spanned(expression, kind.expected()));
        };
        if !closure.inputs.is_empty() {
            return Err(syn::Error::new_spanned(closure, kind.arguments_message()));
        }
        let index = self.closures.len();
        self.closures.push(Closure {
            expression: expression.clone(),
            captured: captured_identifiers(closure),
        });
        Ok(index)
    }
}

fn closure_tokens(closure: &Closure, shared: &BTreeMap<String, usize>) -> TokenStream {
    let expression = &closure.expression;
    let clones: Vec<Ident> = closure
        .captured
        .iter()
        .filter(|identifier| shared.get(*identifier).copied().unwrap_or(0) > 1)
        .map(|identifier| Ident::new(identifier, Span::call_site()))
        .collect();
    if clones.is_empty() {
        return quote! { #expression };
    }
    let binds = clones.iter().map(|identifier| {
        quote! { let #identifier = ::std::clone::Clone::clone(&#identifier); }
    });
    quote! {{
        #(#binds)*
        #expression
    }}
}

#[derive(Clone, Copy)]
enum ClosureKind {
    Text,
    Attribute,
    Event,
}

impl ClosureKind {
    fn expected(self) -> &'static str {
        match self {
            Self::Text => {
                "dynamic text must be a zero-argument closure, for example {move || value}"
            }
            Self::Attribute => {
                "dynamic attribute must be a zero-argument closure, for example aria-label={move || value}"
            }
            Self::Event => {
                "event handler must be a zero-argument closure, for example on:click={move || action()}"
            }
        }
    }

    fn arguments_message(self) -> &'static str {
        match self {
            Self::Text => "dynamic text closure must not take arguments",
            Self::Attribute => "dynamic attribute closure must not take arguments",
            Self::Event => "event closure must not take arguments; use set values through signals",
        }
    }
}

fn ignored_at_root(node: &Node) -> bool {
    match node {
        Node::Comment(_) => true,
        Node::RawText(raw) => raw.to_token_stream_string().trim().is_empty(),
        _ => false,
    }
}

fn block_expression(block: &NodeBlock) -> syn::Result<&Expr> {
    let block = block.try_block().ok_or_else(|| {
        syn::Error::new_spanned(block, "view! block is not a valid Rust expression")
    })?;
    match block.stmts.as_slice() {
        [Stmt::Expr(expression, None)] => Ok(expression),
        _ => Err(syn::Error::new_spanned(
            block,
            "a view! block must contain exactly one expression, for example {move || value}",
        )),
    }
}

fn ungroup(expression: &Expr) -> &Expr {
    match expression {
        Expr::Block(block) => match block.block.stmts.as_slice() {
            [Stmt::Expr(expression, None)] => ungroup(expression),
            _ => expression,
        },
        _ => expression,
    }
}

fn string_literal(expression: &Expr) -> Option<&LitStr> {
    match ungroup(expression) {
        Expr::Lit(ExprLit {
            lit: Lit::Str(literal),
            ..
        }) => Some(literal),
        _ => None,
    }
}

fn captured_identifiers(closure: &ExprClosure) -> BTreeSet<String> {
    let mut collector = CaptureCollector::default();
    collector.visit_expr(&closure.body);
    for bound in collector.bound {
        collector.used.remove(&bound);
    }
    collector.used
}

#[derive(Default)]
struct CaptureCollector {
    used: BTreeSet<String>,
    bound: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for CaptureCollector {
    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        if path.qself.is_none()
            && path.path.leading_colon.is_none()
            && path.path.segments.len() == 1
        {
            self.used.insert(path.path.segments[0].ident.to_string());
        }
        syn::visit::visit_expr_path(self, path);
    }

    fn visit_pat_ident(&mut self, pattern: &'ast syn::PatIdent) {
        self.bound.insert(pattern.ident.to_string());
        syn::visit::visit_pat_ident(self, pattern);
    }

    fn visit_expr_closure(&mut self, closure: &'ast ExprClosure) {
        for input in &closure.inputs {
            collect_pattern_idents(input, &mut self.bound);
        }
        syn::visit::visit_expr_closure(self, closure);
    }
}

fn collect_pattern_idents(pattern: &syn::Pat, bound: &mut BTreeSet<String>) {
    struct Bound<'a>(&'a mut BTreeSet<String>);
    impl<'ast> Visit<'ast> for Bound<'_> {
        fn visit_pat_ident(&mut self, pattern: &'ast syn::PatIdent) {
            self.0.insert(pattern.ident.to_string());
            syn::visit::visit_pat_ident(self, pattern);
        }
    }
    Bound(bound).visit_pat(pattern);
}
