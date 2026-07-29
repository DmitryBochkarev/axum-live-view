use super::*;
use crate as axum_live_view;
use crate::html;
use serde_json::json;

fn pretty_print<T>(t: T) -> T
where
    T: Serialize,
{
    println!("{}", serde_json::to_string_pretty(&t).unwrap());
    t
}

#[test]
fn basic() {
    let view: Html<()> = html! { <div></div> };
    assert_eq!(view.render(), "<div></div>");
}

#[test]
fn doctype() {
    let view: Html<()> = html! { <!DOCTYPE html> };
    assert_eq!(view.render(), "<!DOCTYPE html>");
}

#[test]
fn text() {
    let view: Html<()> = html! { "foo" };
    assert_eq!(view.render(), "foo");
}

#[test]
fn text_inside_tag() {
    let view: Html<()> = html! { <div>"foo"</div> };
    assert_eq!(view.render(), "<div>foo</div>");
}

#[test]
fn interpolate() {
    let count = 1;
    let view: Html<()> = html! { <div>{ count }</div> };
    assert_eq!(view.render(), "<div>1</div>");
}

#[test]
fn fixed_next_to_dynamic() {
    let count = 1;
    let view: Html<()> = html! {
        <div>"foo"</div>
        <div>{ count }</div>
    };
    assert_eq!(view.render(), "<div>foo</div><div>1</div>");
}

#[test]
fn nested_tags() {
    let view: Html<()> = html! {
        <div>
            <p>"foo"</p>
        </div>
    };
    assert_eq!(view.render(), "<div><p>foo</p></div>");
}

#[test]
fn deeply_nested() {
    let count = 1;
    let view: Html<()> = html! {
        <div>
            <ul>
                <li>{ count }</li>
                <li>"2"</li>
                <li>"3"</li>
            </ul>
        </div>
    };
    assert_eq!(
        view.render(),
        "<div><ul><li>1</li><li>2</li><li>3</li></ul></div>"
    );
}

#[test]
fn nested_with_more_html_calls() {
    let view: Html<()> = html! {
        <div>
            <ul>
                {
                    let nested: Html<()> = html! {
                        <li>"1"</li>
                        <li>"2"</li>
                        <li>"3"</li>
                    };
                    nested
                }
            </ul>
        </div>
    };
    assert_eq!(
        view.render(),
        "<div><ul><li>1</li><li>2</li><li>3</li></ul></div>"
    );
}

#[test]
fn attribute() {
    let view: Html<()> = html! {
        <div class="col-md">"foo"</div>
    };
    assert_eq!(view.render(), "<div class=\"col-md\">foo</div>");
}

#[test]
fn multiple_attributes() {
    let view: Html<()> = html! {
        <div class="col-md" id="the-thing">"foo"</div>
    };
    assert_eq!(
        view.render(),
        "<div class=\"col-md\" id=\"the-thing\">foo</div>"
    );
}

#[test]
fn attribute_with_dash() {
    let view: Html<()> = html! {
        <div on-click="do thing">"foo"</div>
    };
    assert_eq!(view.render(), "<div on-click=\"do thing\">foo</div>");
}

#[test]
fn interpolate_class() {
    let size = 8;
    let view: Html<String> = html! {
        <div class={ format!("col-{}", size) }>"foo"</div>
    };
    assert_eq!(view.render(), "<div class=\"col-8\">foo</div>");
}

#[test]
fn empty_attribute() {
    let view: Html<()> = html! {
        <button disabled>"foo"</button>
    };
    assert_eq!(view.render(), "<button disabled>foo</button>");
}

#[test]
fn empty_tag() {
    let view: Html<()> = html! {
        <img src="foo.png" />
    };
    assert_eq!(view.render(), "<img src=\"foo.png\">");
}

#[test]
fn attribute_with_spaces() {
    let view: Html<()> = html! {
        <input placeholder="What needs to be done?" />
    };
    assert_eq!(
        view.render(),
        "<input placeholder=\"What needs to be done?\">"
    );
}

#[test]
fn dynamic_attribute_with_spaces() {
    let class = "todo-text completed";
    let view: Html<()> = html! {
        <span class={ class }></span>
    };
    assert_eq!(view.render(), "<span class=\"todo-text completed\"></span>");
}

#[test]
fn conditional() {
    let view: Html<()> = html! {
        <div>
            if true {
                <p>"some paragraph..."</p>
            }
        </div>
    };
    assert_eq!(view.render(), "<div><p>some paragraph...</p></div>");
}

#[test]
fn conditional_else() {
    let view: Html<()> = html! {
        <div>
            if true {
                <p>"some paragraph..."</p>
            } else {
                <p>"wat"</p>
            }
        </div>
    };
    assert_eq!(view.render(), "<div><p>some paragraph...</p></div>");
}

#[test]
fn conditional_else_if() {
    let view: Html<()> = html! {
        <div>
            if true {
                <p>"some paragraph..."</p>
            } else if false {
                <p>"wat"</p>
            } else {
                <p>"wat"</p>
            }
        </div>
    };
    assert_eq!(view.render(), "<div><p>some paragraph...</p></div>");
}

#[test]
fn conditional_with_single_expr() {
    fn render(x: bool) -> Html<()> {
        html! {
            if x {
                "a"
            }
        }
    }
    assert_eq!(render(true).render(), "a");
    assert_eq!(render(false).render(), "");
}

#[test]
fn if_let() {
    let name = Some("bob");
    let view: Html<()> = html! {
        <div>
            if let Some(name) = name {
                <p>{ format!("Hi {}", name) }</p>
            } else {
                <p>"Missing name..."</p>
            }
        </div>
    };
    assert_eq!(view.render(), "<div><p>Hi bob</p></div>");
}

#[test]
fn for_loop() {
    let names = ["alice", "bob", "cindy"];
    let view: Html<()> = html! {
        <ul>
            for name in names {
                <li>{ name }</li>
            }
        </ul>
    };
    assert_eq!(
        view.render(),
        concat!(
            "<ul>",
            "<li>alice</li>",
            "<li>bob</li>",
            "<li>cindy</li>",
            "</ul>",
        ),
    );
}

#[test]
fn for_loop_with_conditional() {
    let ns = [1, 11, 2];
    let view: Html<()> = html! {
        <ul>
            for n in ns {
                <li>
                if n >= 10 {
                    <strong>"big number"</strong>
                } else {
                    { n }
                }
                </li>
            }
        </ul>
    };
    assert_eq!(
        view.render(),
        concat!(
            "<ul>",
            "<li>1</li>",
            "<li><strong>big number</strong></li>",
            "<li>2</li>",
            "</ul>",
        ),
    );
}

#[test]
fn match_() {
    let name = Some("bob");
    let view: Html<()> = html! {
        <div>
            match name {
                Some(name) => {
                    <p>{ format!("Hi {}", name) }</p>
                },
                None => {
                    <p>"Missing name..."</p>
                },
            }
        </div>
    };
    assert_eq!(view.render(), "<div><p>Hi bob</p></div>");
}

#[test]
fn match_guard() {
    let count = Some(10);
    let view: Html<()> = html! {
        <div>
            match count {
                Some(count) if count == 0 => {
                    <p>"its zero!"</p>
                },
                Some(count) => {
                    <p>{ count }</p>
                },
                None => {
                    <p>"Missing count..."</p>
                },
            }
        </div>
    };
    assert_eq!(view.render(), "<div><p>10</p></div>");
}

#[test]
fn keyword_attribute() {
    let view: Html<()> = html! {
        <input type="text" />
    };
    assert_eq!(view.render(), "<input type=\"text\">");
}

#[test]
fn if_up_front() {
    let content = "bar";
    let view: Html<()> = html! {
        if false {}
        "foo"
        { content }
    };
    assert_eq!(view.render(), "foobar");
}

#[test]
fn if_up_front_nested() {
    let content = "bar";
    let view: Html<()> = html! {
        <div>
            if false {}
            "foo"
            { content }
        </div>
    };
    assert_eq!(view.render(), "<div>foobar</div>");
}

#[test]
fn optional_attribute() {
    let view: Html<()> = html! { <input required=() /> };
    assert_eq!(view.render(), "<input required>");

    let view: Html<()> = html! { <input required=Some(()) /> };
    assert_eq!(view.render(), "<input required>");

    let view: Html<()> = html! { <input required=Some("true") /> };
    assert_eq!(view.render(), "<input required=\"true\">");

    let view: Html<()> = html! { <input required=Some(Some("true")) /> };
    assert_eq!(view.render(), "<input required=\"true\">");

    let view: Html<()> = html! { <input required=Some(Some(None)) /> };
    assert_eq!(view.render(), "<input>");

    let view: Html<()> = html! { <input required=Some(Some({ (1 + 2).to_string() })) /> };
    assert_eq!(view.render(), "<input required=\"3\">");

    let view: Html<()> = html! { <input required=None /> };
    assert_eq!(view.render(), "<input>");

    let view: Html<()> = html! {
        <input required=if true { "true" } />
    };
    assert_eq!(view.render(), "<input required=\"true\">");

    let view: Html<()> = html! {
        <input required=if false { "wat" } else { "true" } />
    };
    assert_eq!(view.render(), "<input required=\"true\">");

    let view: Html<()> = html! {
        <input required=if true { () } />
    };
    assert_eq!(view.render(), "<input required>");

    let view: Html<()> = html! {
        <input required=if false { "wat" } else { () } />
    };
    assert_eq!(view.render(), "<input required>");

    let view: Html<()> = html! {
        <input required=if true { Some(()) } />
    };
    assert_eq!(view.render(), "<input required>");

    let view: Html<()> = html! {
        <input required=if true { None } />
    };
    assert_eq!(view.render(), "<input>");

    let view: Html<()> = html! {
        <input required=if true { Some(()) } else { None } />
    };
    assert_eq!(view.render(), "<input required>");

    let view: Html<()> = html! {
        <input required=if false { Some(()) } else { None } />
    };
    assert_eq!(view.render(), "<input>");

    let view: Html<()> = html! {
        <input required=if true { Some("true") } else { None } />
    };
    assert_eq!(view.render(), "<input required=\"true\">");

    let view: Html<()> = html! {
        <input required=if false { Some("true") } else { None } />
    };
    assert_eq!(view.render(), "<input>");

    let value = Some("true");
    let view: Html<()> = html! {
        <input required=if let Some(value) = value { Some({ value }) } else { None } />
    };
    assert_eq!(view.render(), "<input required=\"true\">");

    let value = None::<String>;
    let view: Html<()> = html! {
        <input required=if let Some(value) = value { Some({ value }) } else { None } />
    };
    assert_eq!(view.render(), "<input>");
}

#[test]
fn axm_attribute() {
    let view: Html<&str> = html! { <input axm-click={ "foo" } /> };
    assert_eq!(view.render(), "<input axm-click=%22foo%22>");

    let view: Html<&str> = html! { <input axm-click=if true { "foo" } else { "bar" } /> };
    assert_eq!(view.render(), "<input axm-click=%22foo%22>");

    let view: Html<Option<&str>> =
        html! { <input axm-click=if true { Some("foo") } else { None } /> };
    assert_eq!(view.render(), "<input axm-click=%22foo%22>");

    #[derive(Serialize)]
    enum Msg {
        Foo,
        Bar { value: i32 },
    }

    let view: Html<Msg> = html! { <input axm-click={ Msg::Foo } /> };
    assert_eq!(view.render(), "<input axm-click=%22Foo%22>");

    let view: Html<Msg> = html! { <input axm-click={ Msg::Bar { value: 123 } } /> };
    assert_eq!(
        view.render(),
        "<input axm-click={%22Bar%22:{%22value%22:123}}>"
    );
}

#[test]
fn axm_enum_update_attribute() {
    #[derive(Serialize)]
    struct Msg {
        n: i32,
    }

    let view = html! { <foo axm-click={ Msg { n: 1 } } /> };
    let json = json!(view);
    println!("{}", serde_json::to_string_pretty(&json).unwrap());
    assert_json_diff::assert_json_eq!(
        json,
        json!({
            "d": {
                "0": "{%22n%22:1}",
            },
            "f": [
                "<foo axm-click=",
                ">",
            ],
        })
    );
}

#[test]
fn diffing_fixed() {
    let old: Html<()> = html! { <div>"old"</div> };
    let new: Html<()> = html! { <div>"new"</div> };
    let diff = old.diff(&new);
    assert_json_diff::assert_json_eq!(
        diff,
        json!({
            "f": ["<div>new</div>"],
        })
    );
}

#[test]
fn diffing_dynamic() {
    fn render(value: i32) -> Html<()> {
        html! { <div>{ value }</div> }
    }
    let old = render(1);
    let new = render(2);
    let diff = old.diff(&new);
    assert_json_diff::assert_json_eq!(
        diff,
        json!({
            "d": {
                "0": "2"
            }
        })
    );
}

#[test]
fn diffing_dynamic_multiple_dynamics() {
    fn render(one: i32, two: i32) -> Html<()> {
        html! { <div>{ one } " and " { two }</div> }
    }

    let a = render(1, 2);

    let b = render(1, 2);
    assert_json_diff::assert_json_eq!(a.diff(&b), json!(null));

    let b = render(2, 2);
    assert_json_diff::assert_json_eq!(
        a.diff(&b),
        json!({
            "d": {
                "0": "2",
            }
        })
    );

    let b = render(2, 3);
    assert_json_diff::assert_json_eq!(
        a.diff(&b),
        json!({
            "d": {
                "0": "2",
                "1": "3",
            }
        })
    );
}

#[test]
fn diffing_dynamic_changing_fixed() {
    fn render(n: i32) -> Html<()> {
        html! {
            <div>{ n }</div>
            if n >= 10 {
                <div>"big number"</div>
            }
        }
    }

    let a = render(1);
    let b = render(11);
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "d": {
                "0": "11",
                "1": {
                    "f": ["<div>big number</div>"],
                }
            },
        })
    );

    let a = render(11);
    let b = render(12);
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "d": {
                "0": "12",
            },
        })
    );
}

#[test]
fn diffing_loop_dynaming_changes() {
    fn render(ns: &[i32]) -> Html<()> {
        html! {
            <ul>
                for n in ns {
                    <li>{ n }</li>
                }
            </ul>
        }
    }

    let a = render(&[1, 2, 3]);
    let b = render(&[1, 2, 3]);
    assert_json_diff::assert_json_eq!(pretty_print(a.diff(&b)), json!(null));

    let a = render(&[1, 2]);
    let b = render(&[3, 4]);
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "d": {
                "0": {
                    "b": {
                        "0": { "0": "3" },
                        "1": { "0": "4" }
                    }
                }
            }
        })
    );

    let a = render(&[1, 2]);
    let b = render(&[2, 2]);
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "d": {
                "0": {
                    "b": {
                        "0": { "0": "2" },
                    }
                }
            }
        })
    );

    let a = render(&[1]);
    let b = render(&[1, 2]);
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "d": {
                "0": {
                    "b": {
                        "1": { "0": "2" },
                    }
                }
            }
        })
    );
}

#[test]
fn diffing_loop_fixed_changes() {
    fn render_one(ns: &[i32]) -> Html<()> {
        html! {
            <ul>
                for n in ns {
                    <li>{ n }</li>
                }
            </ul>
        }
    }

    fn render_two(ns: &[i32]) -> Html<()> {
        html! {
            <ul>
                for n in ns {
                    <li disabled>{ n }</li>
                }
            </ul>
        }
    }

    let a = render_one(&[1, 2, 3]);
    let b = render_two(&[1, 2, 3]);
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "d": {
                "0": {
                    "f": [
                        "<li disabled>",
                        "</li>"
                    ],
                }
            }
        })
    );

    let a = render_one(&[1, 2]);
    let b = render_two(&[1, 2, 3]);
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "d": {
                "0": {
                    "f": [
                        "<li disabled>",
                        "</li>"
                    ],
                    "b": {
                        "2": { "0": "3" }
                    }
                }
            }
        })
    );

    let a = render_one(&[1, 2, 3]);
    let b = render_two(&[1, 2]);
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "d": {
                "0": {
                    "f": [
                        "<li disabled>",
                        "</li>"
                    ],
                    "b": {
                        "2": null
                    }
                }
            }
        })
    );
}

#[test]
fn diffing_removing_dynamic() {
    fn render_one(n: i32, m: i32) -> Html<()> {
        html! {
            { n }
            { m }
        }
    }

    fn render_two(n: i32) -> Html<()> {
        html! {
            { n }
        }
    }

    let a = render_one(1, 2);
    let b = render_two(1);
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "f": ["", ""],
            "d": {
                "1": null,
            }
        })
    );
}

#[test]
fn diffing_loop_conditional() {
    fn render(ns: &[i32]) -> Html<()> {
        html! {
            <ul>
                for n in ns {
                    <li>
                        if *n >= 10 {
                            <strong>"big number"</strong>
                        } else {
                            { n }
                        }
                    </li>
                }
            </ul>
        }
    }

    let a = render(&[1, 2, 3]);
    let b = render(&[1, 11, 3]);
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "d": {
                "0": {
                    "b": {
                        "1": {
                            "0": {
                                "f": ["<strong>big number</strong>"],
                                "d": { "0": null }
                            }
                        },
                    }
                }
            }
        })
    );
}

#[test]
fn diffing_message() {
    fn render(msg: i32) -> Html<i32> {
        html! { <button axm-click={ msg }></button> }
    }

    let a = render(1);
    assert_json_diff::assert_json_eq!(a.diff(&a), json!(null));

    let b = render(2);
    assert_json_diff::assert_json_eq!(
        a.diff(&b),
        json!({
            "d": {
                "0": "2",
            }
        })
    );
}

#[test]
fn diffing_dynamic_or_fixed() {
    fn render(n: i32, m: i32) -> Html<()> {
        html! {
            if n == m {
                <div>{ n }</div>
            } else {
                "not same"
            }
        }
    }

    let a = pretty_print(render(1, 2));
    let b = pretty_print(render(1, 1));
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "d": {
                "0": {
                    "f": ["<div>", "</div>"],
                    "d": { "0": "1" },
                }
            },
        })
    );

    let a = pretty_print(render(1, 1));
    let b = pretty_print(render(1, 2));
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "d": {
                "0": {
                    "f": ["not same"],
                    "d": { "0": null },
                }
            },
        })
    );
}

#[test]
fn starting_with_dynamic() {
    let view: Html<()> = html! {
        if true {
            "one."
        }
        "two"
    };
    assert_eq!(view.render(), "one.two");
}

#[test]
fn match_with_blocks() {
    let view: Html<()> = html! {
        match Some(()) {
            Some(()) => {
                "one"
            },
            None => {
                "two"
            },
        }
    };
    assert_eq!(view.render(), "one");
}

#[test]
fn if_toggle() {
    fn render(flag: bool) -> Html<()> {
        if flag {
            html! { "hi" }
        } else {
            html! {}
        }
    }

    let a = render(true);
    let b = render(false);
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "f": []
        })
    );
}

// ---------------------------------------------------------------------------
// match tests
// ---------------------------------------------------------------------------

#[test]
fn match_nested_html() {
    enum Variant {
        A,
        B,
    }

    fn render(v: Variant) -> Html<()> {
        html! {
            match v {
                Variant::A => {
                    <div class="a">
                        <p>"Alpha"</p>
                    </div>
                },
                Variant::B => {
                    <div class="b">
                        <span>"Beta"</span>
                    </div>
                },
            }
        }
    }

    assert_eq!(
        render(Variant::A).render(),
        "<div class=\"a\"><p>Alpha</p></div>"
    );
    assert_eq!(
        render(Variant::B).render(),
        "<div class=\"b\"><span>Beta</span></div>"
    );
}

#[test]
fn match_multiple_nodes() {
    let view: Html<()> = html! {
        match true {
            true => {
                <p>"first"</p>
                <p>"second"</p>
            },
            false => {
                <p>"nope"</p>
            },
        }
    };
    assert_eq!(view.render(), "<p>first</p><p>second</p>");
}

#[test]
fn match_with_if_inside() {
    let count = 5;
    let view: Html<()> = html! {
        match count {
            n if n > 0 => {
                if n > 10 {
                    <strong>"big"</strong>
                } else {
                    <em>"small"</em>
                }
            },
            _ => {
                <p>"zero"</p>
            },
        }
    };
    assert_eq!(view.render(), "<em>small</em>");
}

#[test]
fn match_with_for_inside() {
    let items = ["a", "b", "c"];
    let view: Html<()> = html! {
        <ul>
            match true {
                true => {
                    for item in items {
                        <li>{ item }</li>
                    }
                },
                false => {
                    <li>"empty"</li>
                },
            }
        </ul>
    };
    assert_eq!(
        view.render(),
        "<ul><li>a</li><li>b</li><li>c</li></ul>"
    );
}

#[test]
fn match_at_root() {
    let kind = "hello";
    let view: Html<()> = html! {
        match kind {
            "hello" => {
                <h1>"Greetings!"</h1>
            },
            _ => {
                <p>"whatever"</p>
            },
        }
    };
    assert_eq!(view.render(), "<h1>Greetings!</h1>");
}

#[test]
fn match_exhaustive_enum() {
    #[derive(Clone, Copy)]
    enum Color {
        Red,
        Green,
        Blue,
    }

    fn render(c: Color) -> Html<()> {
        html! {
            match c {
                Color::Red => {
                    <span style="color:red">"R"</span>
                },
                Color::Green => {
                    <span style="color:green">"G"</span>
                },
                Color::Blue => {
                    <span style="color:blue">"B"</span>
                },
            }
        }
    }

    assert_eq!(
        render(Color::Red).render(),
        "<span style=\"color:red\">R</span>"
    );
    assert_eq!(
        render(Color::Green).render(),
        "<span style=\"color:green\">G</span>"
    );
    assert_eq!(
        render(Color::Blue).render(),
        "<span style=\"color:blue\">B</span>"
    );
}

#[test]
fn match_inside_tag() {
    let kind = 1;
    let view: Html<()> = html! {
        <div>
            match kind {
                1 => {
                    <p>"one"</p>
                },
                2 => {
                    <p>"two"</p>
                },
                _ => {
                    <p>"many"</p>
                },
            }
        </div>
    };
    assert_eq!(view.render(), "<div><p>one</p></div>");
}

#[test]
fn diffing_match_switching_arms() {
    fn render(n: i32) -> Html<()> {
        html! {
            match n {
                0 => {
                    <p>"zero"</p>
                },
                _ => {
                    <p>"non-zero"</p>
                },
            }
        }
    }

    let a = render(0);
    let b = render(1);
    // Each arm produces a self-contained Html; switching arms replaces
    // the entire inner Html, so only the `fixed` part changes.
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "d": {
                "0": {
                    "f": ["<p>non-zero</p>"]
                }
            }
        })
    );
}

#[test]
fn diffing_match_with_dynamic() {
    fn render(n: i32) -> Html<()> {
        html! {
            match n {
                0 => {
                    <p>{ "zero" }</p>
                },
                _ => {
                    <p>{ "non-zero" }</p>
                },
            }
        }
    }

    let a = render(0);
    let b = render(1);
    // When arms use `{ }` (dynamic blocks), the `fixed` parts are the same
    // (`<p></p>`) and only the inner dynamic changes.
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "d": {
                "0": {
                    "d": {
                        "0": "non-zero"
                    }
                }
            }
        })
    );
}

#[test]
fn diffing_match_arm_same_no_diff() {
    fn render(n: i32) -> Html<()> {
        html! {
            match n {
                0 => {
                    <p>"zero"</p>
                },
                _ => {
                    <p>"non-zero"</p>
                },
            }
        }
    }

    let a = render(0);
    let b = render(0);
    assert_json_diff::assert_json_eq!(pretty_print(a.diff(&b)), json!(null));
}

// ---------------------------------------------------------------------------
// if tests
// ---------------------------------------------------------------------------

#[test]
fn if_without_else() {
    fn render(show: bool) -> Html<()> {
        html! {
            if show {
                <p>"visible"</p>
            }
        }
    }
    assert_eq!(render(true).render(), "<p>visible</p>");
    assert_eq!(render(false).render(), "");
}

#[test]
fn if_multiple_nodes() {
    let view: Html<()> = html! {
        if true {
            <p>"first"</p>
            <p>"second"</p>
            <p>"third"</p>
        }
    };
    assert_eq!(view.render(), "<p>first</p><p>second</p><p>third</p>");
}

#[test]
fn if_with_for_inside() {
    let items = [1, 2, 3];
    let view: Html<()> = html! {
        if true {
            for item in items {
                <span>{ item }</span>
            }
        }
    };
    assert_eq!(view.render(), "<span>1</span><span>2</span><span>3</span>");
}

#[test]
fn if_with_match_inside() {
    let view: Html<()> = html! {
        if true {
            match 2 {
                1 => {
                    <p>"one"</p>
                },
                2 => {
                    <p>"two"</p>
                },
                _ => {
                    <p>"other"</p>
                },
            }
        }
    };
    assert_eq!(view.render(), "<p>two</p>");
}

#[test]
fn if_nested() {
    let outer = true;
    let inner = false;
    let view: Html<()> = html! {
        if outer {
            if inner {
                <p>"both"</p>
            } else {
                <p>"outer only"</p>
            }
        }
    };
    assert_eq!(view.render(), "<p>outer only</p>");
}

#[test]
fn diffing_if_condition_change() {
    fn render(flag: bool) -> Html<()> {
        html! {
            if flag {
                <p>"yes"</p>
            } else {
                <p>"no"</p>
            }
        }
    }

    let a = render(true);
    let b = render(false);
    // Literal text inside tags is baked into `fixed`, so the entire
    // fixed part changes when switching branches.
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "d": {
                "0": {
                    "f": ["<p>no</p>"]
                }
            }
        })
    );
}

#[test]
fn diffing_if_with_dynamic() {
    fn render(flag: bool) -> Html<()> {
        html! {
            if flag {
                <p>{ "yes" }</p>
            } else {
                <p>{ "no" }</p>
            }
        }
    }

    let a = render(true);
    let b = render(false);
    // With `{ }` blocks the fixed parts stay the same; only the
    // inner dynamic fragment changes.
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "d": {
                "0": {
                    "d": {
                        "0": "no"
                    }
                }
            }
        })
    );
}

#[test]
fn diffing_if_multiple_nodes_change() {
    fn render(flag: bool) -> Html<()> {
        html! {
            if flag {
                <p>"a"</p>
                <p>"b"</p>
            } else {
                <span>"single"</span>
            }
        }
    }

    let a = render(true);
    let b = render(false);
    // When the branch changes, the entire nested Html is replaced;
    // the `fixed` part of the inner Html reflects the new branch content.
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "d": {
                "0": {
                    "f": ["<span>single</span>"]
                }
            }
        })
    );
}

// ---------------------------------------------------------------------------
// for tests
// ---------------------------------------------------------------------------

#[test]
fn for_loop_empty() {
    fn render(items: &[i32]) -> Html<()> {
        html! {
            <ul>
                for item in items {
                    <li>{ item }</li>
                }
            </ul>
        }
    }
    assert_eq!(render(&[]).render(), "<ul></ul>");
}

#[test]
fn for_loop_single() {
    let view: Html<()> = html! {
        <ul>
            for x in [42] {
                <li>{ x }</li>
            }
        </ul>
    };
    assert_eq!(view.render(), "<ul><li>42</li></ul>");
}

#[test]
fn for_loop_nested() {
    let xs = [1, 2];
    let ys = ["a", "b"];
    let view: Html<()> = html! {
        <table>
            for x in xs {
                <tr>
                    for y in ys {
                        <td>{ x }{ y }</td>
                    }
                </tr>
            }
        </table>
    };
    assert_eq!(
        view.render(),
        concat!(
            "<table>",
            "<tr><td>1a</td><td>1b</td></tr>",
            "<tr><td>2a</td><td>2b</td></tr>",
            "</table>",
        )
    );
}

#[test]
fn for_loop_with_match_inside() {
    let items = [1, 2, 3];
    let view: Html<()> = html! {
        <ul>
            for item in items {
                match item % 2 {
                    0 => {
                        <li class="even">{ item }</li>
                    },
                    _ => {
                        <li class="odd">{ item }</li>
                    },
                }
            }
        </ul>
    };
    assert_eq!(
        view.render(),
        concat!(
            "<ul>",
            "<li class=\"odd\">1</li>",
            "<li class=\"even\">2</li>",
            "<li class=\"odd\">3</li>",
            "</ul>",
        )
    );
}

#[test]
fn for_loop_with_dynamic_attribute() {
    let items = [1, 2];
    let view: Html<()> = html! {
        <ul>
            for item in items {
                <li data-index={ item }>{ item }</li>
            }
        </ul>
    };
    assert_eq!(
        view.render(),
        concat!("<ul>", "<li data-index=\"1\">1</li>", "<li data-index=\"2\">2</li>", "</ul>",)
    );
}

#[test]
fn for_loop_multiple_dynamics() {
    let items = [(1, "a"), (2, "b")];
    let view: Html<()> = html! {
        <ul>
            for (n, s) in items {
                <li>
                    <strong>{ n }</strong>
                    ": "
                    <em>{ s }</em>
                </li>
            }
        </ul>
    };
    assert_eq!(
        view.render(),
        concat!(
            "<ul>",
            "<li><strong>1</strong>: <em>a</em></li>",
            "<li><strong>2</strong>: <em>b</em></li>",
            "</ul>",
        )
    );
}

#[test]
fn for_loop_no_container() {
    let items = ["x", "y"];
    let view: Html<()> = html! {
        for item in items {
            <span>{ item }</span>
        }
    };
    assert_eq!(view.render(), "<span>x</span><span>y</span>");
}

#[test]
fn diffing_for_loop_length_change() {
    fn render(ns: &[i32]) -> Html<()> {
        html! {
            <ul>
                for n in ns {
                    <li>{ n }</li>
                }
            </ul>
        }
    }

    // adding an iteration
    let a = render(&[1, 2]);
    let b = render(&[1, 2, 3]);
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "d": {
                "0": {
                    "b": {
                        "2": { "0": "3" }
                    }
                }
            }
        })
    );

    // removing an iteration
    let a = render(&[1, 2, 3]);
    let b = render(&[1, 2]);
    assert_json_diff::assert_json_eq!(
        pretty_print(a.diff(&b)),
        json!({
            "d": {
                "0": {
                    "b": {
                        "2": null
                    }
                }
            }
        })
    );
}
