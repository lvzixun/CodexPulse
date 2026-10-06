import { mount } from 'svelte';
import App from './App.svelte';
import './styles/app.css';
import { refreshLanguage } from './lib/system-language';

async function start() {
  await refreshLanguage();
  const element = document.getElementById('app');
  if (element) mount(App, { target: element });
}
void start();
