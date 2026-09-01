use crate::ConversationArea;

pub const GENERAL_TALK_AREA_ID: &str = "general-talk";

const BUILT_IN_AREAS: [(&str, &str); 6] = [
    (GENERAL_TALK_AREA_ID, "General Talk"),
    ("brick-copper", "Brick & Copper"),
    ("vine-branch-deli", "Vine and Branch Deli"),
    ("sb-dom-online-ai", "S&B / Dom / Online AI"),
    ("personal", "Personal"),
    ("religious-biblical", "Religious / Biblical"),
];

pub fn built_in_conversation_areas() -> Vec<ConversationArea> {
    BUILT_IN_AREAS
        .iter()
        .enumerate()
        .map(|(position, (id, display_name))| ConversationArea {
            id: (*id).to_owned(),
            display_name: (*display_name).to_owned(),
            position: position as u8,
        })
        .collect()
}

pub fn is_valid_conversation_area(area_id: &str) -> bool {
    BUILT_IN_AREAS.iter().any(|(id, _)| *id == area_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_areas_have_stable_ids_and_order() {
        let areas = built_in_conversation_areas();
        assert_eq!(areas.len(), 6);
        assert_eq!(areas[0].id, GENERAL_TALK_AREA_ID);
        assert_eq!(areas[5].display_name, "Religious / Biblical");
        assert!(is_valid_conversation_area("personal"));
        assert!(!is_valid_conversation_area("future-area"));
    }
}
