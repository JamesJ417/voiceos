use voiceos_core::ConversationStore;

#[test]
fn floor_moves_between_devices_but_keeps_one_conversation() {
    let store = ConversationStore::in_memory().unwrap();
    let owner = "owner-1";
    let conversation = store
        .resolve_owner_conversation(owner, "pixel-1", Some("phone-session"))
        .unwrap();
    let panel_conversation = store
        .resolve_owner_conversation(owner, "panel-1", Some("panel-session"))
        .unwrap();
    assert_eq!(conversation, panel_conversation);

    let phone = store
        .change_conversation_floor(
            owner,
            &conversation,
            "pixel-1",
            Some("Pixel"),
            "claim",
            Some("listening"),
            Some("what are we working on"),
            None,
            45,
        )
        .unwrap();
    assert!(phone.active);
    assert_eq!(phone.holder_device_id.as_deref(), Some("pixel-1"));

    let panel = store
        .change_conversation_floor(
            owner,
            &conversation,
            "panel-1",
            Some("Home panel"),
            "claim",
            Some("listening"),
            None,
            None,
            45,
        )
        .unwrap();
    assert_eq!(panel.conversation_id, conversation);
    assert_eq!(panel.holder_device_id.as_deref(), Some("panel-1"));
    assert!(panel.revision > phone.revision);

    let stale_phone_update = store.change_conversation_floor(
        owner,
        &conversation,
        "pixel-1",
        Some("Pixel"),
        "update",
        Some("speaking"),
        None,
        Some("This must stay silent."),
        45,
    );
    assert_eq!(
        stale_phone_update.unwrap_err().to_string(),
        "invalid agent record: conversation_floor_not_owned"
    );

    let released = store
        .change_conversation_floor(
            owner,
            &conversation,
            "panel-1",
            Some("Home panel"),
            "release",
            Some("idle"),
            None,
            None,
            45,
        )
        .unwrap();
    assert!(!released.active);
    assert_eq!(released.phase, "idle");
    assert!(released.holder_device_id.is_none());
}
