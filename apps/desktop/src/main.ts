import { mount } from 'svelte';
import App from './App.svelte';
import './styles/app.css';
const element = document.getElementById('app');
if (element) mount(App, { target: element });
