use crabclaw::channels::base::ChannelResponse;
use crabclaw::core::agent_loop::LoopResult;

pub fn assert_ok_reply(response: &ChannelResponse, expected: &str) {
    assert!(
        response.error.is_none(),
        "unexpected error: {:?}",
        response.error
    );
    assert_eq!(response.assistant_output.as_deref(), Some(expected));
}

pub fn assert_has_error(response: &ChannelResponse) {
    assert!(response.error.is_some(), "expected error in response");
}

pub fn assert_no_error_channel(response: &ChannelResponse) {
    assert!(
        response.error.is_none(),
        "unexpected error: {:?}",
        response.error
    );
}

pub fn assert_non_empty_channel_output(response: &ChannelResponse) {
    assert_no_error_channel(response);
    let output = response.assistant_output.as_deref().unwrap_or("");
    assert!(!output.is_empty(), "expected non-empty assistant output");
}

pub fn assert_no_error_loop(result: &LoopResult) {
    assert!(result.error.is_none(), "unexpected error: {:?}", result.error);
}

pub fn assert_non_empty_loop_output(result: &LoopResult) {
    assert_no_error_loop(result);
    let output = result.assistant_output.as_deref().unwrap_or("");
    assert!(!output.is_empty(), "expected non-empty assistant output");
}
