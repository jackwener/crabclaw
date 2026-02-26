pub fn text_response(content: &str) -> String {
    format!(
        r#"{{"choices":[{{"message":{{"role":"assistant","content":"{content}"}},"finish_reason":"stop"}}]}}"#
    )
}

pub fn tool_call_response(tool_name: &str, call_id: &str, arguments: &str) -> String {
    let args_escaped = arguments.replace('"', "\\\"");
    format!(
        r#"{{"choices":[{{"message":{{"role":"assistant","content":"","tool_calls":[{{"id":"{call_id}","type":"function","function":{{"name":"{tool_name}","arguments":"{args_escaped}"}}}}]}},"finish_reason":"tool_calls"}}]}}"#
    )
}
