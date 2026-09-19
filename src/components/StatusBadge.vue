<script setup lang="ts">
import { computed } from 'vue'
import { labelForStatus, toneForStatus } from '../theme'

const props = defineProps<{ value: unknown; label?: string }>()
const tone = computed(() => toneForStatus(props.value))
const text = computed(() => props.label || labelForStatus(props.value))
</script>

<template>
  <span class="status-badge" :class="`tone-${tone}`">
    <span class="status-dot" aria-hidden="true" />
    {{ text }}
  </span>
</template>
