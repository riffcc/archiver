import { defineStore } from 'pinia';
import { ref } from 'vue';

export interface Track {
  id: string;
  title: string;
  artist?: string;
  album?: string;
  duration: number;
  url: string;
}

export const usePlayerStore = defineStore('player', () => {
  const currentTrack = ref<Track | null>(null);
  const isPlaying = ref(false);
  const currentTime = ref(0);
  const volume = ref(1);
  const queue = ref<Track[]>([]);

  const play = () => {
    isPlaying.value = true;
  };

  const pause = () => {
    isPlaying.value = false;
  };

  const togglePlayPause = () => {
    isPlaying.value = !isPlaying.value;
  };

  const setTrack = (track: Track) => {
    currentTrack.value = track;
    currentTime.value = 0;
  };

  const setVolume = (newVolume: number) => {
    volume.value = Math.max(0, Math.min(1, newVolume));
  };

  const seek = (time: number) => {
    currentTime.value = time;
  };

  return {
    currentTrack,
    isPlaying,
    currentTime,
    volume,
    queue,
    play,
    pause,
    togglePlayPause,
    setTrack,
    setVolume,
    seek
  };
});