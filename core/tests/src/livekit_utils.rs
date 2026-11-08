use livekit_api::access_token;

pub fn generate_token(name: &str) -> String {
    let room = std::env::var("HOPP_TEST_ROOM").unwrap_or_else(|_| "dev_room".into());
    generate_room_token(name, &room)
}

pub fn generate_room_token(name: &str, room: &str) -> String {
    let api_key = std::env::var("LIVEKIT_API_KEY").unwrap();
    let api_secret = std::env::var("LIVEKIT_API_SECRET").unwrap();

    access_token::AccessToken::with_api_key(&api_key, &api_secret)
        .with_identity(name)
        .with_name(name)
        .with_grants(access_token::VideoGrants {
            room_join: true,
            room: room.to_string(),
            ..Default::default()
        })
        .to_jwt()
        .unwrap()
}
