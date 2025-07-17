<template>
  <div id="app" :data-theme="theme">
    <header class="header">
      <h1>Librarian</h1>
      <div class="header-controls">
        <button @click="toggleTheme" class="btn btn-secondary">
          {{ theme === 'dark' ? '☀️' : '🌙' }}
        </button>
        <button @click="openSettings" class="btn btn-secondary">
          Settings
        </button>
      </div>
    </header>
    
    <main class="container">
      <div class="search-bar">
        <input 
          v-model="searchQuery"
          type="text" 
          placeholder="Search collections or items..."
          class="search-input"
        />
      </div>
      
      <div class="grid-tiles">
        <div 
          v-for="item in filteredItems" 
          :key="item.id"
          class="tile"
          :class="{ 
            visited: item.visited, 
            selected: item.selected 
          }"
          @click="handleTileClick(item)"
        >
          <div class="tile-content">
            <h3>{{ item.title }}</h3>
            <p v-if="item.artist">{{ item.artist }}</p>
            <p v-if="item.year">{{ item.year }}</p>
          </div>
          <div v-if="item.selected" class="selection-indicator">✓</div>
        </div>
      </div>
    </main>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { useSettingsStore } from './stores/settings';
import { useCollectionsStore } from './stores/collections';

const settingsStore = useSettingsStore();
const collectionsStore = useCollectionsStore();

const theme = computed(() => settingsStore.settings.theme);
const searchQuery = ref('');

// Mock data for testing
const mockItems = ref([
  { id: '1', title: '78 RPM Collection', artist: 'Various Artists', year: 1920, visited: false, selected: false, type: 'collection' as const },
  { id: '2', title: 'Jazz Archives', artist: 'Duke Ellington', year: 1935, visited: true, selected: false, type: 'collection' as const },
  { id: '3', title: 'Classical Recordings', artist: 'Berlin Philharmonic', year: 1960, visited: false, selected: false, type: 'collection' as const },
  { id: '4', title: 'Folk Music Treasury', artist: 'Various Artists', year: 1965, visited: false, selected: false, type: 'collection' as const },
  { id: '5', title: 'Blues Masters', artist: 'B.B. King', year: 1970, visited: true, selected: false, type: 'collection' as const },
  { id: '6', title: 'World Music', artist: 'Various Artists', year: 1980, visited: false, selected: false, type: 'collection' as const },
]);

const filteredItems = computed(() => {
  const query = searchQuery.value.toLowerCase();
  if (!query) return mockItems.value;
  
  return mockItems.value.filter(item => 
    item.title.toLowerCase().includes(query) ||
    (item.artist && item.artist.toLowerCase().includes(query))
  );
});

const toggleTheme = () => {
  settingsStore.toggleTheme();
};

const openSettings = () => {
  console.log('Opening settings...');
  // TODO: Implement settings modal/page
};

const handleTileClick = (item: any) => {
  item.selected = !item.selected;
  if (!item.visited) {
    item.visited = true;
  }
};

onMounted(() => {
  collectionsStore.setItems(mockItems.value);
});
</script>

<style>
.search-bar {
  padding: 1rem;
  background-color: var(--bg-secondary);
  border-bottom: 1px solid var(--border);
}

.search-input {
  width: 100%;
  padding: 0.75rem 1rem;
  background-color: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: 8px;
  color: var(--text-primary);
  font-size: 1rem;
  transition: border-color 0.2s ease;
}

.search-input:focus {
  outline: none;
  border-color: var(--accent);
}

.search-input::placeholder {
  color: var(--text-tertiary);
}

.header-controls {
  display: flex;
  gap: 1rem;
}

.tile-content {
  flex: 1;
}

.tile-content h3 {
  font-size: 1.125rem;
  margin-bottom: 0.5rem;
  color: var(--text-primary);
}

.tile-content p {
  font-size: 0.875rem;
  color: var(--text-secondary);
  margin-bottom: 0.25rem;
}

.selection-indicator {
  position: absolute;
  top: 0.5rem;
  right: 0.5rem;
  width: 24px;
  height: 24px;
  background-color: var(--accent);
  color: white;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  font-weight: bold;
}

main {
  flex: 1;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.grid-tiles {
  flex: 1;
  overflow-y: auto;
}
</style>