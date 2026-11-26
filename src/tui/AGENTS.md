# Terminal interface

Follow the [TUI design principles](../../docs/tui-design.md) for all terminal screens.

Render full-screen pages without an outer frame. Use a separate title row. Prompts may use borders; place the title on its own line clear of the border. Keep drawing and mouse hit testing aligned with the same layout.

Use bold active-theme-colored titles. Fill the next row with a horizontal separator in the active theme color, then begin content on the following row. Editable controls show the insertion cursor in a distinct color. All clickable buttons must be reachable with the arrow keys or Tab and activated by Enter. Use Escape to go back; omit dedicated back buttons. Explain user-visible setup choices in terms of their purpose and consequences.
Arrow keys follow the on-screen geometry and stop at edges. Tab and Shift+Tab cycle in reading order. For multi-decision setup, use a step-by-step wizard with short plain-language instructions and retain earlier answers when navigating back.

In application-authored Chinese interface copy, use an ASCII period followed by one ASCII space (U+002E U+0020) for a sentence-ending period. Never use U+3002, including at the end of a label or message.

## Control design

- For a required decision, distribute the choices across the entire available action row. For actions the user can enter or invoke, group buttons against the left edge. Give each button twelve terminal columns and center its label within its hit area. Measure terminal display columns, including full-width characters.
- Show each button as its working shortcut, a space, and its name. Place all action buttons on the penultimate row, with F1 through F10 in page order. Limit each page to ten buttons.
- Show the current focus's available operations on the last row. Format hints as `space · 切换同步模式`. Update them when focus moves. Quick reply is a contextual `r` action on a selected published letter.
- Draw every unchecked checkbox as □ and every checked checkbox as ■ in the applied theme color. Keep the square distinguishable when its label has focus. For a commit-producing checkbox, Space stages and Enter submits. In multi-select lists Space toggles an item and Enter finishes the entire selection.
- Escape returns one level. Preserve the active preferences tab when leaving a nested editor or picker. Input focus in the compose page highlights only the field identifier and its padding. Give the body identifier its own line.
- Frame the workspace tree as an untitled control inside the borderless page. Distinguish tagged directories with both a visible marker and color.
