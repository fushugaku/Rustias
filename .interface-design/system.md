# Rustias browser instrument

The silver RADIAS faceplate, dark fluted knobs, blue LCD and red note pads are the visual signature. The browser shows controls implemented by the firmware-free native profile: four timbres, oscillator, filter, amplifier envelope, level/pan and sixteen chromatic pads. Group controls by signal flow.

Canvas #202327, app surface #282c30, faceplate #cbd0d5, panel ink #262b30, pad #da535c, LCD #c7dfeb. Text hierarchy #e5e7e9 / #aeb4bb / #89929b / #626b75. App border #3e444b, panel border #a4adb5, focus #c7e1f3 on dark controls and #3e6380 on the panel. Use 4 px spacing units and 4 px corners; only physical dials and LEDs are circular. Arial/Helvetica for panel lettering, system monospace for values/status.

Flat panel groups with dividing rules; only the recessed LCD and knobs use shadows. No decorative cards or invented firmware displays. Four modules form one signal path. At narrower widths use two columns; at mobile widths group the envelope controls in two columns and show pads in a four-column grid. Touch controls remain at least 44 px. Monitor status reflects the actual AudioWorklet counters.

Dial interaction: vertical drag, Shift for fine adjustment, wheel, arrow keys, Page Up/Down, Home/End and double-click reset. Pads support multiple pointers and release on pointer cancellation/focus loss. Keep CSS tokens and controls shared across timbres.
