
        <script setup lang="ts">
        import { ref } from 'vue'
        const stacks = ref([])
        </script>
        <template>
            <div v-for="({ file: efile }) of stacks"></div>
        </template>
        