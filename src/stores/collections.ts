import { defineStore } from 'pinia';
import { ref } from 'vue';

export interface CollectionItem {
  id: string;
  title: string;
  artist?: string;
  year?: number;
  coverUrl?: string;
  type: 'collection' | 'item';
  visited: boolean;
  selected: boolean;
  metadata?: Record<string, any>;
}

export const useCollectionsStore = defineStore('collections', () => {
  const items = ref<CollectionItem[]>([]);
  const loading = ref(false);
  const searchQuery = ref('');
  const currentCollection = ref<string | null>(null);
  const sortBy = ref<'title' | 'date' | 'artist'>('title');

  const setItems = (newItems: CollectionItem[]) => {
    items.value = newItems;
  };

  const toggleItemSelection = (itemId: string) => {
    const item = items.value.find(i => i.id === itemId);
    if (item) {
      item.selected = !item.selected;
    }
  };

  const markAsVisited = (itemId: string) => {
    const item = items.value.find(i => i.id === itemId);
    if (item) {
      item.visited = true;
    }
  };

  const getSelectedItems = () => {
    return items.value.filter(item => item.selected);
  };

  const clearSelection = () => {
    items.value.forEach(item => {
      item.selected = false;
    });
  };

  return {
    items,
    loading,
    searchQuery,
    currentCollection,
    sortBy,
    setItems,
    toggleItemSelection,
    markAsVisited,
    getSelectedItems,
    clearSelection
  };
});