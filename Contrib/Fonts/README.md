# Password sentence mask

The password box uses English translations of the eight sentences from the original Chinese control (introduced in commit `ba51fa432afd8fc6fa4b3d7b40421d130ba33e81`; last implementation before removal at `38a92cf05ac09e2b76e5e0f40f58c2f425401b09`). Creation and confirmation use the same fixed translation. Other password fields shuffle all eight translations independently of the password and show their prefix as the user types, with spaces between sentences.

Only the text presenter's rendered layout is masked. The bound password, native editing, selection, input composition and reveal button retain their current behavior. Clipboard operations and accessibility values do not expose a hidden passphrase.

The English mask inherits the application's font family, style, weight and stretch. The former Chinese-only font subset and its build script are no longer needed. Interactive headless checks verify every translated character has a glyph in the application font, alongside native editing, Unicode input, reveal and clipboard/accessibility protection.
