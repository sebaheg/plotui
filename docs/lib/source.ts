import { loader } from 'fumadocs-core/source';
import { docs } from 'collections/server';

export const source = loader(docs.toFumadocsSource(), {
  baseUrl: '/docs',
});
