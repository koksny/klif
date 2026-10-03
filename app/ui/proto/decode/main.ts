import { mount } from 'svelte';
import '@fontsource/iosevka/300.css';
import '@fontsource/iosevka/500.css';
import '@fontsource/iosevka/700.css';
import Proto from './Proto.svelte';

mount(Proto, { target: document.getElementById('app')! });
