// SRE composition only: approved Wallet owns keys, authentication and UI semantics.
import { LocalKey } from '../../../web/s3/key.ts';
import { LocalClient } from '../../../web/s3/client.ts';
import { mount } from '../../../web/s3/component.ts';
import { context, type Context } from '../../../web/s3/state.ts';
import { base64 } from '../../../web/s3/direct-codec.ts';

export class TabEntry {
  #keys: LocalKey[] = [];
  #mounted?: ReturnType<typeof mount>;
  #closed = false;
  readonly origin: string;
  constructor(origin: string, enabled = false, acknowledge = false) {
    if (enabled !== true || acknowledge !== true) throw Error('TWO_OPT_INS_REQUIRED');
    if (!['http://127.0.0.1:5173', 'http://localhost:5173'].includes(origin)) throw Error('ORIGIN_REJECTED');
    this.origin = origin;
    try { for (let i = 0; i < 2; i++) this.#keys.push(new LocalKey()); }
    catch (e) { this.destroy(); throw e; }
  }
  // Only public registration data leaves this object. No fixture seeds/storage.
  registrations() {
    if (this.#closed) throw Error('TAB_CLOSED');
    return this.#keys.map(k => ({owner: k.owner, address: k.address, public_key_base64: base64.encode(k.publicKey)}));
  }
  activate(root: HTMLElement, supplied: Context, pinned: Context, transport: typeof fetch) {
    if (this.#closed || this.#mounted) throw Error('TAB_CLOSED_OR_ACTIVE');
    // pinned is supplied by the reviewed launcher, never inferred from an API response.
    context(supplied, pinned);
    if (root.ownerDocument.defaultView?.location.origin !== this.origin) throw Error('DOCUMENT_ORIGIN');
    const ctx = Object.freeze({...pinned});
    const client = LocalClient.authenticated(ctx, transport, true, true);
    try {
      this.#mounted = mount(root, client, this.#keys, this.origin);
      root.ownerDocument.defaultView?.addEventListener('pagehide', () => this.destroy(), {once: true});
    } catch (e) { client.destroy(); this.destroy(); throw e; }
  }
  destroy() {
    if (this.#closed) return;
    this.#closed = true;
    this.#mounted?.destroy();
    this.#keys.forEach(k => k.destroy());
    this.#keys = [];
  }
}
