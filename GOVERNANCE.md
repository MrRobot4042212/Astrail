# Gobernanza de Meteor

Meteor es un proyecto de un solo mantenedor. Este documento dice quién decide qué,
para que quien contribuye sepa a qué atenerse y para que cualquier tercero que
firme o distribuya las versiones oficiales sepa de dónde salen.

## Roles

**Mantenedor y autor: Diego Alfonso Chicoma Ibañez (Dalfon.dev)** —
[@MrRobot4042212](https://github.com/MrRobot4042212).

Es el autor original del proyecto y tiene la última palabra sobre el diseño, lo
que entra y lo que no, y las publicaciones. En concreto:

- Revisa y aprueba todos los cambios. `.github/CODEOWNERS` lo refleja.
- Es el único con permiso de escritura en `master` y en `deploy`, y el único que
  publica versiones.
- Custodia las claves y los secretos del repositorio: la clave de firma del
  actualizador (`TAURI_SIGNING_PRIVATE_KEY`), las credenciales de IGDB y, cuando
  exista, el certificado de firma de código.

**Colaboradores.** Cualquiera puede abrir issues y pull requests. No hace falta
ningún acuerdo previo: no hay CLA y cada persona conserva el copyright de lo que
escribe (ver [CONTRIBUTING.md](CONTRIBUTING.md)).

Si en algún momento hay más mantenedores, se listarán aquí junto a su ámbito; el
mantenedor original conserva la decisión final mientras el proyecto siga siendo
suyo.

## Cómo se decide

1. Se discute en el issue o en el PR, en público.
2. El mantenedor decide y explica por qué. Un «no» razonado es una respuesta
   completa: no todo cambio encaja en el alcance del proyecto.
3. Las decisiones que afectan al diseño quedan escritas en el repositorio (en
   `CONTRIBUTING.md`, en los comentarios del código o en `docs/`), no solo en el
   hilo.

Hay decisiones que no se revisan porque son parte de lo que Meteor es: sin
inyección de DLL en procesos de juegos, sin componentes propietarios, y sin
telemetría.

## Publicaciones

Solo el mantenedor publica. Una versión oficial es la que sale del workflow
`release.yml` de este repositorio (`MrRobot4042212/Meteor`) al fusionar en
`deploy`: construye el instalador, lo firma y publica la release de GitHub que lee
el actualizador integrado.

Cualquier otra compilación es una versión no oficial, y la pantalla *Ajustes →
Acerca de* lo dice, como pide el término 2 de
[ADDITIONAL-TERMS.md](ADDITIONAL-TERMS.md).

## Seguridad

Los fallos de seguridad se comunican en privado al mantenedor a través de
[GitHub Security Advisories](https://github.com/MrRobot4042212/Meteor/security/advisories/new),
no en un issue público.

## Licencia

GPL-3.0-only, con los términos adicionales de la sección 7 recogidos en
[ADDITIONAL-TERMS.md](ADDITIONAL-TERMS.md). Cambiar la licencia del proyecto
requeriría el permiso de todas las personas que hayan aportado código, porque no
hay CLA que ceda esos derechos a nadie.
