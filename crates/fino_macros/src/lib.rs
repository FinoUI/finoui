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
use quote::quote;
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

impl Default for Builder {
    fn default() -> Self {
        Self {
            template: Vec::new(),
            bindings: Vec::new(),
            closures: Vec::new(),
            text_slots: Vec::new(),
            next_node: 0,
            template_ident: hidden_ident("__fino_template"),
            nodes_ident: hidden_ident("__fino_nodes"),
            node_idents: Vec::new(),
            name: None,
        }
    }
}

struct Builder {
    template: Vec<TokenStream>,
    bindings: Vec<Binding>,
    closures: Vec<Closure>,
    text_slots: Vec<usize>,
    next_node: usize,
    template_ident: Ident,
    nodes_ident: Ident,
    node_idents: Vec<Ident>,
    name: Option<String>,
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
        self.node_idents
            .push(hidden_ident(&format!("__fino_node_{node}")));
        node
    }

    fn node_ident(&self, node: usize) -> Ident {
        self.node_idents[node].clone()
    }

    fn finish(self) -> TokenStream {
        let Builder {
            template,
            bindings,
            closures,
            text_slots,
            next_node: _,
            template_ident,
            nodes_ident,
            node_idents: _,
            name: _,
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
                        .mount(runtime, owner, &#nodes_ident[#node]);
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
                    .mount(runtime, owner, &#nodes_ident[#node]);
                },
                Binding::Event { node, closure: _ } => quote! {
                    ::fino::view::EventBinding::click(#expression)
                        .mount(runtime, owner, &#nodes_ident[#node]);
                },
            }
        });
        let slots = text_slots
            .iter()
            .map(|slot| quote! { #nodes_ident[#slot].clone() });

        quote! {{
            let mut #template_ident = ::fino::view::Template::new();
            #(#template)*
            let #nodes_ident = #template_ident.mount(runtime);
            #(#bindings)*
            ::fino::view::ViewOutput::new(
                #nodes_ident[0].clone(),
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
        let node_ident = self.node_ident(node);
        let template_ident = self.template_ident.clone();
        let parent = match parent {
            Some(parent) => {
                let parent_ident = self.node_ident(parent);
                quote! { Some(#parent_ident) }
            }
            None => quote! { None },
        };
        self.template.push(quote! {
            let #node_ident = #template_ident.element(
                #parent,
                ::fino::dom_api::Tag::#tag,
            );
        });

        let mut errors = None;
        let mut events = BTreeSet::new();
        for attribute in element.attributes() {
            if let Err(error) = self.attribute(attribute, node, &mut events) {
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

    fn attribute(
        &mut self,
        attribute: &NodeAttribute,
        node: usize,
        events: &mut BTreeSet<String>,
    ) -> syn::Result<()> {
        let NodeAttribute::Attribute(attribute) = attribute else {
            return Err(syn::Error::new_spanned(
                attribute,
                "dynamic attribute keys are not supported by view!",
            ));
        };
        let key = attribute.key.to_string();
        if key == "fino:name" {
            if node != 0 {
                return Err(syn::Error::new_spanned(
                    &attribute.key,
                    "fino:name is only supported on the root view element",
                ));
            }
            if self.name.is_some() {
                return Err(syn::Error::new_spanned(
                    &attribute.key,
                    "fino:name is specified more than once",
                ));
            }
            let value = attribute.value().ok_or_else(|| {
                syn::Error::new_spanned(&attribute.key, "fino:name requires a string value")
            })?;
            let literal = string_literal(value).ok_or_else(|| {
                syn::Error::new_spanned(value, "fino:name requires a static string literal")
            })?;
            self.name = Some(literal.value());
            return Ok(());
        }
        if let Some(event) = key.strip_prefix("on:") {
            if event != "click" {
                return Err(syn::Error::new_spanned(
                    &attribute.key,
                    format!("unsupported event `{key}`; view! supports only on:click"),
                ));
            }
            if !events.insert(key.clone()) {
                return Err(syn::Error::new_spanned(
                    &attribute.key,
                    format!("duplicate event `{key}` on one element"),
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
                    format!(
                        "unsupported attribute `{key}`; view! supports aria-label and root fino:name"
                    ),
                ));
            }
        };
        let attribute_ident = Ident::new(attribute_name, attribute.key.span());
        let value = attribute.value().ok_or_else(|| {
            syn::Error::new_spanned(&attribute.key, format!("{key} requires a value"))
        })?;

        if let Some(literal) = string_literal(value) {
            let node_ident = self.node_ident(node);
            let template_ident = self.template_ident.clone();
            self.template.push(quote! {
                #template_ident.static_attribute(
                    #node_ident,
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
        let node_ident = self.node_ident(node);
        let parent_ident = self.node_ident(parent);
        let template_ident = self.template_ident.clone();
        self.template.push(quote! {
            let #node_ident = #template_ident.text(#parent_ident, #literal);
        });
        Ok(())
    }

    fn dynamic_text(&mut self, block: &NodeBlock, parent: usize) -> syn::Result<()> {
        let expression = block_expression(block)?;
        let closure = self.closure(expression, ClosureKind::Text)?;
        let node = self.next_node();
        let node_ident = self.node_ident(node);
        let parent_ident = self.node_ident(parent);
        let template_ident = self.template_ident.clone();
        self.template.push(quote! {
            let #node_ident = #template_ident.text_slot(#parent_ident);
        });
        let name = match &self.name {
            Some(prefix) if self.text_slots.is_empty() => format!("{prefix}/text"),
            Some(prefix) => format!("{prefix}/text/{node}"),
            None => format!("view/text/{node}"),
        };
        self.text_slots.push(node);
        let name = LitStr::new(&name, expression.span());
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

fn hidden_ident(name: &str) -> Ident {
    Ident::new(name, Span::mixed_site())
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
            Self::Event => "event closure must not take arguments; write values through signals",
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
    collector.push_scope();
    for input in &closure.inputs {
        collector.bind_pattern(input);
    }
    collector.visit_expr(&closure.body);
    collector.pop_scope();
    collector.used
}

#[derive(Default)]
struct CaptureCollector {
    used: BTreeSet<String>,
    scopes: Vec<BTreeSet<String>>,
}

impl CaptureCollector {
    fn push_scope(&mut self) {
        self.scopes.push(BTreeSet::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn is_bound(&self, identifier: &str) -> bool {
        self.scopes
            .iter()
            .rev()
            .any(|scope| scope.contains(identifier))
    }

    fn bind_pattern(&mut self, pattern: &syn::Pat) {
        let mut names = BTreeSet::new();
        collect_pattern_idents(pattern, &mut names);
        if let Some(scope) = self.scopes.last_mut() {
            scope.extend(names);
        }
    }
}

impl<'ast> Visit<'ast> for CaptureCollector {
    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        if path.qself.is_none()
            && path.path.leading_colon.is_none()
            && path.path.segments.len() == 1
        {
            let identifier = path.path.segments[0].ident.to_string();
            if !self.is_bound(&identifier) {
                self.used.insert(identifier);
            }
        }
        syn::visit::visit_expr_path(self, path);
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.push_scope();
        for statement in &block.stmts {
            self.visit_stmt(statement);
        }
        self.pop_scope();
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        // The initializer is evaluated before the new binding exists.
        // 初始化表达式先于新绑定生效，因此先访问 initializer。
        if let Some(initializer) = &local.init {
            self.visit_expr(&initializer.expr);
            if let Some((_else_token, diverge)) = &initializer.diverge {
                self.visit_expr(diverge);
            }
        }
        self.bind_pattern(&local.pat);
    }

    fn visit_expr_closure(&mut self, closure: &'ast ExprClosure) {
        self.push_scope();
        for input in &closure.inputs {
            self.bind_pattern(input);
        }
        self.visit_expr(&closure.body);
        self.pop_scope();
    }

    fn visit_expr_for_loop(&mut self, for_loop: &'ast syn::ExprForLoop) {
        self.visit_expr(&for_loop.expr);
        self.push_scope();
        self.bind_pattern(&for_loop.pat);
        self.visit_block(&for_loop.body);
        self.pop_scope();
    }

    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        self.push_scope();
        self.bind_pattern(&arm.pat);
        self.visit_expr(&arm.body);
        self.pop_scope();
    }

    fn visit_expr_macro(&mut self, mac: &'ast syn::ExprMacro) {
        let Some(name) = mac
            .mac
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
        else {
            return;
        };
        if !is_expression_macro(&name) {
            return;
        }
        // Every macro argument is an expression in these macros, so parse the
        // arguments before visiting them instead of treating tokens as names.
        // 这些宏的每个参数都是表达式；先解析参数，再按表达式访问其中的标识符。
        let parser = syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated;
        if let Ok(arguments) = mac.mac.parse_body_with(parser) {
            for argument in arguments {
                self.visit_expr(&argument);
            }
        }
    }
}

fn is_expression_macro(name: &str) -> bool {
    matches!(
        name,
        "assert"
            | "assert_eq"
            | "assert_ne"
            | "dbg"
            | "debug_assert"
            | "debug_assert_eq"
            | "debug_assert_ne"
            | "eprint"
            | "eprintln"
            | "format"
            | "format_args"
            | "panic"
            | "print"
            | "println"
            | "todo"
            | "unimplemented"
            | "unreachable"
            | "vec"
            | "write"
            | "writeln"
    )
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
