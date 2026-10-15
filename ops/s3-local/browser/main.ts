import { attachPage } from './page.ts';
attachPage(document, window.fetch.bind(window));
