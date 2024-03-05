struct ExampleDesc {
    name: &'static str,
    function: fn(),
    #[allow(dead_code)] // isn't used on native
    webgl: bool,
    #[allow(dead_code)] // isn't used on native
    webgpu: bool,
}

const EXAMPLES: &[ExampleDesc] = &[
    ExampleDesc {
        name: "triangle",
        function: fast3d_examples::triangle::main,
        webgl: true,
        webgpu: true,
    }
];

fn get_example_name() -> Option<String> {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            let query_string = web_sys::window()?.location().search().ok()?;

            fast3d_examples::framework::parse_url_query_string(&query_string, "example").map(String::from)
        } else {
            std::env::args().nth(1)
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn print_examples() {
    // Get the document, header, and body elements.
    let document = web_sys::window().unwrap().document().unwrap();

    for backend in ["webgl2", "webgpu"] {
        let ul = document
            .get_element_by_id(&format!("{backend}-list"))
            .unwrap();

        for example in EXAMPLES {
            if backend == "webgl2" && !example.webgl {
                continue;
            }
            if backend == "webgpu" && !example.webgpu {
                continue;
            }

            let link = document.create_element("a").unwrap();
            link.set_text_content(Some(example.name));
            link.set_attribute(
                "href",
                &format!("?backend={backend}&example={}", example.name),
            )
                .unwrap();
            link.set_class_name("example-link");

            let item = document.create_element("div").unwrap();
            item.append_child(&link).unwrap();
            item.set_class_name("example-item");
            ul.append_child(&item).unwrap();
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn print_unknown_example(_result: Option<String>) {}

#[cfg(not(target_arch = "wasm32"))]
fn print_unknown_example(result: Option<String>) {
    if let Some(example) = result {
        println!("Unknown example: {}", example);
    } else {
        println!("Please specify an example as the first argument!");
    }

    println!("\nAvailable Examples:");
    for examples in EXAMPLES {
        println!("\t{}", examples.name);
    }
}

fn main() {
    #[cfg(target_arch = "wasm32")]
    print_examples();

    let Some(example) = get_example_name() else {
        print_unknown_example(None);
        return;
    };

    let Some(found) = EXAMPLES.iter().find(|e| e.name == example) else {
        print_unknown_example(Some(example));
        return;
    };

    (found.function)();
}