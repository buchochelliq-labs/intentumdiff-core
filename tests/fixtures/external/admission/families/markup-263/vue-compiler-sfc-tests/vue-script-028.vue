<script setup>
        import { ref } from 'vue'
        const count = ref(0)
        const maybe = foo()
        let lett = 1
        </script>
        <template>
          <input v-model="count">
          <input v-model="maybe">
          <input v-model="lett">
        </template>
        