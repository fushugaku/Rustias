pub type Connection = midir::MidiInputConnection<()>;
pub fn virtual_input(
    mut receive: impl FnMut(&[u8]) + Send + 'static,
) -> Result<Connection, String> {
    let mut input = midir::MidiInput::new("RADIAS Rust").map_err(|e| e.to_string())?;
    input.ignore(midir::Ignore::None);
    #[cfg(unix)]
    {
        use midir::os::unix::VirtualInput;
        input
            .create_virtual(
                "RADIAS Rust",
                move |_, bytes, _| {
                    receive(bytes);
                },
                (),
            )
            .map_err(|e| e.to_string())
    }
    #[cfg(not(unix))]
    {
        let _ = receive;
        Err("Виртуальный MIDI-вход доступен в macOS/Linux".into())
    }
}
