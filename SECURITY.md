# Politique de sécurité — étape de cadrage

**Ravum n'est pas un compilateur, runtime ou produit déployable aujourd'hui.** Aucun SLA ni support de production n'est annoncé. Les futures fonctions d'analyse, de preuves, de patch natif et de compilation pourront être sensibles.

Éviter de publier des exploits actifs, secrets, tokens, fichiers utilisateurs ou informations personnelles dans les issues publiques. Signaler de manière responsable un problème de sécurité au mainteneur du dépôt par les mécanismes privés GitHub lorsqu'ils sont disponibles.

Une vulnérabilité de l'outillage expérimental ne doit pas être « corrigée » en désactivant un test, une preuve, un refus d'ABI ou une vérification de contrat.

Les frontières `unsafe`, FFI, compilateurs et solveurs tiers sont des bases de confiance explicitement auditables. Les obligations de preuve ne deviennent pas `PROVED` parce qu'un scan ou un test a réussi.
