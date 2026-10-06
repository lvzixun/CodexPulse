import 'svelte/elements';

// WebKit's native macOS switch; older engines keep an accessible checkbox.
// https://webkit.org/blog/15054/an-html-switch-control/
declare module 'svelte/elements' {
  interface HTMLInputAttributes {
    switch?: boolean;
  }
}
