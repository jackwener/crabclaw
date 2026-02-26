pub fn sse_stream(chunks: &[&str]) -> String {
    let mut body = String::new();
    for chunk in chunks {
        body.push_str(&format!("data: {chunk}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    body
}

pub fn sse_content_chunk(text: &str) -> String {
    format!(r#"{{"choices":[{{"delta":{{"content":"{text}"}},"finish_reason":null}}]}}"#)
}

pub fn sse_tool_call_start(index: usize, id: &str, name: &str) -> String {
    format!(
        r#"{{"choices":[{{"delta":{{"tool_calls":[{{"index":{index},"id":"{id}","function":{{"name":"{name}","arguments":""}}}}]}},"finish_reason":null}}]}}"#
    )
}

pub fn sse_tool_call_args(index: usize, args: &str) -> String {
    let args_escaped = args.replace('"', "\\\"");
    format!(
        r#"{{"choices":[{{"delta":{{"tool_calls":[{{"index":{index},"function":{{"arguments":"{args_escaped}"}}}}]}},"finish_reason":null}}]}}"#
    )
}
