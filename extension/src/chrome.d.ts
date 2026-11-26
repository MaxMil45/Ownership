// The few extension APIs this extension uses (Chrome and Edge, Manifest V3).
// Written by hand to keep the build free of extra packages.

declare namespace chrome {
  namespace storage {
    const local: {
      get(keys: string[]): Promise<Record<string, unknown>>;
      set(items: Record<string, unknown>): Promise<void>;
    };
  }
  namespace runtime {
    const onInstalled: { addListener(cb: () => void): void };
    function getURL(path: string): string;
    function openOptionsPage(): Promise<void>;
  }
  namespace contextMenus {
    interface OnClickData {
      menuItemId: string | number;
      selectionText?: string;
    }
    function create(p: { id: string; title: string; contexts: string[] }): void;
    function removeAll(): Promise<void>;
    const onClicked: { addListener(cb: (info: OnClickData) => void): void };
  }
  namespace tabs {
    function create(p: { url: string }): Promise<unknown>;
  }
  namespace omnibox {
    const onInputEntered: { addListener(cb: (text: string) => void): void };
    function setDefaultSuggestion(s: { description: string }): void;
  }
}
