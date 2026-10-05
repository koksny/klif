// klif-webui's entry (webui.html): a separate, small page the KLIF engine serves on the LAN.
import { mount } from 'svelte';
import './webui.css';
import App from './App.svelte';

async function start() {
  // On the Vite dev server, `?mock` answers the API in the browser (never part of a build).
  if (import.meta.env.DEV && new URLSearchParams(location.search).has('mock')) {
    const { installMock } = await import('./mock');
    installMock();
  }
  mount(App, { target: document.getElementById('app')! });
}

void start();
