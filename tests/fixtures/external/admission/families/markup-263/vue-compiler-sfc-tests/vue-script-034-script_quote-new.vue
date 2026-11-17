
        <script setup>
        import { ref } from "vue"
        const count = ref(0)
        const style = { color: 'red' }
        const height = ref(0)
        </script>
        <template>
          <div>{{ count }}</div>
          <div>static</div>
        </template>
        <style>
        div { color: v-bind(count) }
        span { color: v-bind(style.color) }
        span { color: v-bind(height + "px") }
        </style>
        