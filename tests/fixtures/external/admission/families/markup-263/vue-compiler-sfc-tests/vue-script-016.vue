<script setup>
      import { ref } from 'vue'
      defineProps(['foo'])
      defineEmits(['bar'])
      const r = ref(0)
      </script>