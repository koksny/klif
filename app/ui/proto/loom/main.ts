import { mount } from 'svelte';
import '@fontsource/vt323/index.css';
import '@fontsource/ibm-plex-mono/400.css';
import '@fontsource/ibm-plex-mono/500.css';
import Proto from './Proto.svelte';

mount(Proto, { target: document.getElementById('app')! });
