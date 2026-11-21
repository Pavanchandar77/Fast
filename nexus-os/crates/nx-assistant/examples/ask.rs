//! Dev CLI: `cargo run -p nx-assistant --example ask -- "set volume to 30"`.
//! Runs the real local model and prints the chosen tool plus timing.
//! Pass `--exec` to run the full agent (safe tools execute; confirms are declined).

use nx_assistant::{AgentEvent, Assistant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exec = args.iter().any(|a| a == "--exec");
    let prompts: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let a = Assistant::spawn(true);
    for p in prompts {
        if !exec {
            let t = std::time::Instant::now();
            let tool = route_only(p);
            println!("{p:?} -> {tool} ({:.2}s)", t.elapsed().as_secs_f32());
            continue;
        }
        a.ask(p.clone());
        loop {
            match a.events.recv() {
                Ok(AgentEvent::Token(t)) => print!("{t}"),
                Ok(AgentEvent::Proposed {
                    tool,
                    needs_confirm,
                    via,
                }) => {
                    println!(
                        "\n{p:?} -> {tool:?} via {} confirm={needs_confirm}",
                        via.label()
                    );
                    if needs_confirm {
                        a.confirm(false);
                    }
                }
                Ok(AgentEvent::Done {
                    message,
                    stats,
                    sources,
                    ..
                }) => {
                    println!("\nDONE: {message}");
                    for s in sources {
                        println!("  src: {} {}", s.title, s.url);
                    }
                    if let Some(s) = stats {
                        println!(
                            "  prompt {} tok, prefill {:.2}s, {} new tok @ {:.1} tok/s, total {:.2}s",
                            s.prompt_tokens,
                            s.prefill_secs,
                            s.new_tokens,
                            s.tokens_per_sec,
                            s.total_secs
                        );
                    }
                    break;
                }
                Ok(AgentEvent::Error(e)) => {
                    println!("\nERROR: {e}");
                    break;
                }
                Ok(AgentEvent::Phase(ph, _)) => eprintln!("[{ph:?}]"),
                Err(_) => return,
            }
        }
    }
}

fn route_only(prompt: &str) -> String {
    use nx_assistant::llm::LocalModel;
    thread_local! {
        static M: std::cell::RefCell<Option<LocalModel>> = const { std::cell::RefCell::new(None) };
    }
    M.with(|m| {
        let mut m = m.borrow_mut();
        if m.is_none() {
            *m = Some(LocalModel::load(&nx_assistant::model_store::model_path()).expect("model"));
        }
        let model = m.as_mut().expect("model");
        let prefill = "<tool_call>\n{\"name\": \"";
        let g = model
            .chat(
                &nx_assistant::agent::router_system(),
                prompt,
                prefill,
                96,
                &["</tool_call>"],
                |_| true,
            )
            .expect("gen");
        let parsed = nx_assistant::tools::parse_tool_call(&format!("{prefill}{}", g.text));
        format!(
            "{:?} | raw={:?} | {} prompt tok, prefill {:.2}s, {} tok @ {:.1} tok/s",
            parsed,
            g.text,
            g.prompt_tokens,
            g.prefill_secs,
            g.new_tokens,
            g.tokens_per_sec()
        )
    })
}
