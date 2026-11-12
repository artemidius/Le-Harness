# Le Harness v0.1.0-rc.9

Le Harness now remembers the Mistral chat and FIM models selected in the
terminal picker. Subsequent chats reuse the saved choices without repeating
the model dialog. The API key continues to use the operating system credential
store; model IDs are saved separately in the user config directory.

Enter `/model` in a cloud chat to discover available models and choose a new
chat and FIM pair. The new routes apply immediately, preserve the existing
conversation, and are saved for the next launch. Explicit
`LE_HARNESS_CHAT_MODEL` and `LE_HARNESS_FIM_MODEL` environment values still
take precedence at startup.

The terminal acceptance fixture verifies the initial selection, in-chat
switch, outbound request on the new model, and restart without a model picker.
The config store has schema validation and atomic writes.

This is a prerelease. Model availability depends on the authenticated account.
Native command execution is bounded but is not a security sandbox.
