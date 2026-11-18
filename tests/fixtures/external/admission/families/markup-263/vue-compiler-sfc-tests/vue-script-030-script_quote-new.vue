<script setup>
        import { ref } from "vue"
        const count = ref(0)
        </script>
        <script>
        export default {
          data() { return { foo: 123 } }
        }
        </script>
        <template>
          <input v-model="foo">
        </template>
        