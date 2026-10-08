# Rustias browser instrument

The silver RADIAS faceplate, dark fluted knobs, blue LCD and red note pads are the visual signature. The browser exposes the complete implemented native synthesis profile: four timbres, OSC1/OSC2, mixer, both filters, waveshapers, EG1–3, LFO1/2, six patches, voice/pitch, MIDI/tuning and sixteen drum instruments. The performance panel stays above a grouped editor generated from the Rust parameter schema. Group controls by signal flow.

Canvas #202327, app surface #282c30, faceplate #cbd0d5, panel ink #262b30, pad #da535c, LCD #c7dfeb. Text hierarchy #e5e7e9 / #aeb4bb / #89929b / #626b75. App border #3e444b, panel border #a4adb5, focus #c7e1f3 on dark controls and #3e6380 on the panel. Use 4 px spacing units and 4 px corners; only physical dials and LEDs are circular. Arial/Helvetica for panel lettering, system monospace for values/status.

Flat panel groups with dividing rules; only the recessed LCD and knobs use shadows. No decorative cards or invented firmware displays. Four modules form one signal path. At narrower widths use two columns; at mobile widths group the envelope controls in two columns and show pads in a four-column grid. Touch controls remain at least 44 px. Monitor status reflects the actual AudioWorklet counters.

Dial interaction: vertical drag, Shift for fine adjustment, wheel, arrow keys, Page Up/Down, Home/End and double-click reset. Pads support multiple pointers and release on pointer cancellation/focus loss. Keep CSS tokens and controls shared across timbres.

Complete editor: six category tabs, signal-flow sections separated by rules, four desktop columns and two mobile columns. Every select/number field is 44 px high; range/number pairs share a row. Mobile tabs use a 3×2 grid. Parameter dependency states come from current synthesis routing and mode. Global channel can be inherited per timbre; Unison gain bank is a readout owned by allocation. Save/Open operate on all four timbres and sixteen instruments together.
