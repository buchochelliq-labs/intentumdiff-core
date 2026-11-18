<script setup lang="ts">
      import { toRef } from 'vue'
      const props = defineProps<{foo: string}>()
      const foo = toRef(() => props.foo)
      </script>