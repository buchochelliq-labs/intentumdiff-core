<script setup>
        import { bar } from './bar'
        defineProps({
          foo: {
            default: () => bar
          }
        })
        defineEmits({
          foo: () => bar > 1
        })
        </script>