// Shared prop shapes of the Tune drawer components.

/** One entry of a ⋯ menu. `checked` makes it a check item. */
export interface MenuItem {
  label: string;
  onselect: () => void;
  disabled?: boolean;
  /** Why it is disabled, or what it does (tooltip). */
  hint?: string;
  checked?: boolean;
  danger?: boolean;
}
