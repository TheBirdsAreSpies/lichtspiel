// What the player plays, kept outside the player's page (App.vue shows the player from it). With picture in
// picture on, leaving the player's page (back, or the picture-in-picture button) only hides the big player:
// the video plays on in the browser's small window while one looks around ("small"), as in the classic app.
import { reactive } from 'vue';
import type { LocationQuery } from 'vue-router';

export const playing = reactive({ id: '', query: {} as LocationQuery, small: false });
